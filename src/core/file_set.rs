//! File set for collecting files to analyze.

use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use super::progress::{create_spinner, is_tty};
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

        let spinner = if is_tty() {
            let s = create_spinner("Scanning files...");
            Some(s)
        } else {
            None
        };

        let walker = WalkBuilder::new(&root)
            .hidden(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        // Pre-compile glob patterns once for efficient matching
        let exclude_globs = build_glob_set(&exclude_patterns);

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

            // Check exclude patterns using pre-compiled glob set
            let path_str = path.to_string_lossy();
            if exclude_globs.is_match(&*path_str) {
                continue;
            }

            files.push(path.to_path_buf());

            // Update spinner periodically
            if let Some(ref s) = spinner {
                if files.len() % 100 == 0 {
                    s.set_message(format!("Scanning files... {} found", files.len()));
                }
            }
        }

        // Sort for deterministic ordering
        files.sort();

        if let Some(s) = spinner {
            s.finish_with_message(format!("Found {} source files", files.len()));
        }

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

/// Build a compiled GlobSet from a list of patterns.
/// Invalid patterns are silently skipped.
fn build_glob_set(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
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

    #[test]
    fn test_file_set_root() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        assert_eq!(file_set.root(), temp.path().canonicalize().unwrap());
    }

    #[test]
    fn test_file_set_files() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "").unwrap();
        std::fs::write(temp.path().join("b.rs"), "").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        assert_eq!(file_set.files().len(), 2);
    }

    #[test]
    fn test_file_set_iter() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("test.py"), "").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        assert_eq!(file_set.iter().count(), 1);
    }

    #[test]
    fn test_file_set_into_iter() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("test.java"), "").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let files: Vec<_> = file_set.into_iter().collect();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_file_set_ref_into_iter() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("test.js"), "").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let files: Vec<_> = (&file_set).into_iter().collect();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_file_set_group_by_language() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("a.rs"), "").unwrap();
        std::fs::write(temp.path().join("b.rs"), "").unwrap();
        std::fs::write(temp.path().join("c.py"), "").unwrap();

        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let groups = file_set.group_by_language();

        assert_eq!(groups.get(&Language::Rust).map(|v| v.len()), Some(2));
        assert_eq!(groups.get(&Language::Python).map(|v| v.len()), Some(1));
    }

    #[test]
    fn test_file_set_relative_path() {
        let temp = tempfile::tempdir().unwrap();
        let subdir = temp.path().join("src");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("main.rs"), "").unwrap();

        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let abs_path = subdir.join("main.rs").canonicalize().unwrap();
        let rel_path = file_set.relative_path(&abs_path);

        assert!(rel_path.to_string_lossy().contains("main.rs"));
    }

    #[test]
    fn test_file_set_relative_path_outside_root() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("test.rs"), "").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();

        let outside_path = PathBuf::from("/tmp/other/file.rs");
        let rel_path = file_set.relative_path(&outside_path);
        assert_eq!(rel_path, outside_path);
    }

    #[test]
    fn test_file_set_from_path_with_config() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("main.rs"), "").unwrap();
        let config = Config::default();
        let file_set = FileSet::from_path(temp.path(), &config).unwrap();
        assert_eq!(file_set.len(), 1);
    }

    #[test]
    fn test_file_set_with_exclude_patterns() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        let target_dir = temp.path().join("target");
        std::fs::create_dir(&src_dir).unwrap();
        std::fs::create_dir(&target_dir).unwrap();
        std::fs::write(src_dir.join("main.rs"), "").unwrap();
        std::fs::write(target_dir.join("generated.rs"), "").unwrap();

        let file_set =
            FileSet::from_path_with_patterns(temp.path(), vec!["**/target/**".to_string()])
                .unwrap();

        // Only src/main.rs should be included
        assert_eq!(file_set.len(), 1);
        let files: Vec<_> = file_set.iter().collect();
        assert!(files[0].to_string_lossy().contains("main.rs"));
    }

    #[test]
    fn test_file_set_clone() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("test.rs"), "").unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let cloned = file_set.clone();
        assert_eq!(cloned.len(), file_set.len());
        assert_eq!(cloned.root(), file_set.root());
    }

    #[test]
    fn test_file_set_debug() {
        let temp = tempfile::tempdir().unwrap();
        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let debug_str = format!("{:?}", file_set);
        assert!(debug_str.contains("FileSet"));
    }

    #[test]
    fn test_file_set_sorted() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("z.rs"), "").unwrap();
        std::fs::write(temp.path().join("a.rs"), "").unwrap();
        std::fs::write(temp.path().join("m.rs"), "").unwrap();

        let file_set = FileSet::from_path_default(temp.path()).unwrap();
        let files: Vec<_> = file_set.files().iter().collect();

        // Files should be sorted
        for i in 0..files.len() - 1 {
            assert!(files[i] < files[i + 1]);
        }
    }
}
