//! File set for collecting files to analyze.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use super::{Language, Result};
use crate::config::Config;

/// A set of files to analyze, respecting .gitignore.
#[derive(Debug, Clone)]
pub struct FileSet {
    /// Root directory.
    root: PathBuf,
    /// All files in the set.
    files: Vec<PathBuf>,
    /// Excluded patterns.
    #[allow(dead_code)]
    exclude_patterns: Vec<String>,
}

impl FileSet {
    /// Create a file set from a directory path.
    pub fn from_path(path: impl AsRef<Path>, config: &Config) -> Result<Self> {
        Self::from_path_with_patterns(path, config.exclude_patterns.clone())
    }

    /// Create a file set from a directory path without config.
    pub fn from_path_default(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_path_with_patterns(path, Vec::new())
    }

    /// Create a file set with custom exclude patterns.
    pub fn from_path_with_patterns(
        path: impl AsRef<Path>,
        exclude_patterns: Vec<String>,
    ) -> Result<Self> {
        let root = path.as_ref().canonicalize()?;
        let mut files = Vec::new();

        let walker = WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        let exclude_set: HashSet<_> = exclude_patterns.iter().collect();

        for entry in walker.flatten() {
            let path = entry.path();

            // Skip directories
            if path.is_dir() {
                continue;
            }

            // Skip non-source files
            if Language::detect(path).is_none() {
                continue;
            }

            // Check exclude patterns
            let path_str = path.to_string_lossy();
            let should_exclude = exclude_set.iter().any(|pattern| {
                globset::Glob::new(pattern)
                    .ok()
                    .and_then(|g| g.compile_matcher().is_match(&*path_str).then_some(()))
                    .is_some()
            });

            if should_exclude {
                continue;
            }

            files.push(path.to_path_buf());
        }

        // Sort for deterministic ordering
        files.sort();

        Ok(Self {
            root,
            files,
            exclude_patterns,
        })
    }

    /// Get the root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get all files in the set.
    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    /// Get the number of files.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Check if the file set is empty.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Iterate over files.
    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter()
    }

    /// Filter files by language.
    pub fn filter_by_language(&self, lang: Language) -> Vec<&PathBuf> {
        self.files
            .iter()
            .filter(|f| Language::detect(f) == Some(lang))
            .collect()
    }

    /// Get files grouped by language.
    pub fn group_by_language(&self) -> std::collections::HashMap<Language, Vec<&PathBuf>> {
        let mut groups = std::collections::HashMap::new();
        for file in &self.files {
            if let Some(lang) = Language::detect(file) {
                groups.entry(lang).or_insert_with(Vec::new).push(file);
            }
        }
        groups
    }

    /// Get relative path from root.
    pub fn relative_path(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

impl IntoIterator for FileSet {
    type Item = PathBuf;
    type IntoIter = std::vec::IntoIter<PathBuf>;

    fn into_iter(self) -> Self::IntoIter {
        self.files.into_iter()
    }
}

impl<'a> IntoIterator for &'a FileSet {
    type Item = &'a PathBuf;
    type IntoIter = std::slice::Iter<'a, PathBuf>;

    fn into_iter(self) -> Self::IntoIter {
        self.files.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_set_empty() {
        let temp = tempfile::tempdir().unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        assert!(file_set.is_empty());
        assert_eq!(file_set.len(), 0);
    }

    #[test]
    fn test_file_set_with_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("main.go"), "package main").unwrap();
        std::fs::write(temp.path().join("lib.rs"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("README.md"), "# README").unwrap();

        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        assert_eq!(file_set.len(), 2);

        let go_files = file_set.filter_by_language(Language::Go);
        assert_eq!(go_files.len(), 1);

        let rust_files = file_set.filter_by_language(Language::Rust);
        assert_eq!(rust_files.len(), 1);
    }
}
