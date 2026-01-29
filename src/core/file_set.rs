//! File set for collecting files to analyze.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::{WalkBuilder, WalkState};

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

    /// Create a file set from an existing list of files.
    /// Files are sorted for deterministic ordering.
    pub fn from_files(root: PathBuf, files: Vec<PathBuf>) -> Self {
        let mut files = files;
        files.sort();
        Self {
            root,
            files,
            exclude_patterns: Vec::new(),
        }
    }

    /// Create a file set from a TreeSource (git tree at a specific commit).
    /// Only includes files with recognized source code languages.
    pub fn from_tree_source(tree_source: &super::TreeSource, config: &Config) -> Result<Self> {
        let all_files = tree_source.list_files()?;

        // Pre-compile glob patterns for exclusion
        let exclude_globs = build_glob_set(&config.exclude_patterns);

        // Filter to supported languages and apply exclusions
        let files: Vec<PathBuf> = all_files
            .into_iter()
            .filter(|path| {
                // Only include files with recognized languages
                if Language::detect(path).is_none() {
                    return false;
                }
                // Check exclude patterns
                let path_str = path.to_string_lossy();
                !exclude_globs.is_match(&*path_str)
            })
            .collect();

        // Use a placeholder root since files are relative paths from tree
        let root = PathBuf::from(".");
        Ok(Self::from_files(root, files))
    }

    /// Create a file set with custom exclude patterns.
    pub fn from_path_with_patterns(
        path: impl AsRef<Path>,
        exclude_patterns: Vec<String>,
    ) -> Result<Self> {
        let root = path.as_ref().canonicalize()?;

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
            .build_parallel();

        // Pre-compile glob patterns once for efficient matching
        let exclude_globs = build_glob_set(&exclude_patterns);

        let files_mutex = Mutex::new(Vec::new());
        walker.run(|| {
            let files_mutex = &files_mutex;
            let exclude_globs = &exclude_globs;
            let spinner = &spinner;
            Box::new(move |entry| {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => return WalkState::Continue,
                };

                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return WalkState::Continue;
                }

                let path = entry.path();

                if Language::detect(path).is_none() {
                    return WalkState::Continue;
                }

                let path_str = path.to_string_lossy();
                if exclude_globs.is_match(&*path_str) {
                    return WalkState::Continue;
                }

                let owned = entry.into_path();
                let mut locked = files_mutex.lock().expect("file_set mutex poisoned");
                locked.push(owned);

                if let Some(ref s) = spinner {
                    let count = locked.len();
                    if count % 100 == 0 {
                        s.set_message(format!("Scanning files... {count} found"));
                    }
                }

                WalkState::Continue
            })
        });
        let mut files = files_mutex.into_inner().expect("file_set mutex poisoned");

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

    /// Filter files by glob pattern, returning a new FileSet.
    pub fn filter_by_glob(&self, pattern: &str) -> Self {
        let glob = match Glob::new(pattern) {
            Ok(g) => g.compile_matcher(),
            Err(_) => return self.clone(),
        };

        let files: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|f| {
                let path_str = f.to_string_lossy();
                glob.is_match(&*path_str)
                    || f.file_name()
                        .map(|n| glob.is_match(n.to_string_lossy().as_ref()))
                        .unwrap_or(false)
            })
            .cloned()
            .collect();

        Self {
            root: self.root.clone(),
            files,
            exclude_patterns: self.exclude_patterns.clone(),
        }
    }

    /// Exclude files matching glob pattern, returning a new FileSet.
    pub fn exclude_by_glob(&self, pattern: &str) -> Self {
        let glob = match Glob::new(pattern) {
            Ok(g) => g.compile_matcher(),
            Err(_) => return self.clone(),
        };

        let files: Vec<PathBuf> = self
            .files
            .iter()
            .filter(|f| {
                let path_str = f.to_string_lossy();
                !glob.is_match(&*path_str)
                    && !f
                        .file_name()
                        .map(|n| glob.is_match(n.to_string_lossy().as_ref()))
                        .unwrap_or(false)
            })
            .cloned()
            .collect();

        Self {
            root: self.root.clone(),
            files,
            exclude_patterns: self.exclude_patterns.clone(),
        }
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
    fn test_file_set_from_files() {
        let root = PathBuf::from("/project");
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("tests/test.rs"),
        ];

        let file_set = FileSet::from_files(root.clone(), files.clone());

        assert_eq!(file_set.root(), &root);
        assert_eq!(file_set.len(), 3);
        assert!(file_set.files().contains(&PathBuf::from("src/main.rs")));
    }

    #[test]
    fn test_file_set_from_files_sorted() {
        let root = PathBuf::from("/project");
        let files = vec![
            PathBuf::from("z.rs"),
            PathBuf::from("a.rs"),
            PathBuf::from("m.rs"),
        ];

        let file_set = FileSet::from_files(root, files);
        let sorted_files: Vec<_> = file_set.files().to_vec();

        assert_eq!(sorted_files[0], PathBuf::from("a.rs"));
        assert_eq!(sorted_files[1], PathBuf::from("m.rs"));
        assert_eq!(sorted_files[2], PathBuf::from("z.rs"));
    }

    #[test]
    fn test_file_set_from_tree_source() {
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

        // Create source files and non-source files
        std::fs::write(temp.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(temp.path().join("lib.py"), "print('hello')").unwrap();
        std::fs::write(temp.path().join("README.md"), "# readme").unwrap();
        std::fs::write(temp.path().join("data.json"), "{}").unwrap();

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
        let file_set = FileSet::from_tree_source(&tree_source, &config).unwrap();

        // Should only include source files (rs, py), not md or json
        assert_eq!(file_set.len(), 2);
        assert!(file_set
            .files()
            .iter()
            .any(|f| f.to_string_lossy().contains("main.rs")));
        assert!(file_set
            .files()
            .iter()
            .any(|f| f.to_string_lossy().contains("lib.py")));
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

    #[test]
    fn test_filter_by_glob_exact_filename() {
        let root = PathBuf::from("/project");
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/error.rs"),
        ];

        let file_set = FileSet::from_files(root, files);
        let filtered = file_set.filter_by_glob("error.rs");

        assert_eq!(filtered.len(), 1);
        assert!(filtered.files()[0].to_string_lossy().contains("error.rs"));
    }

    #[test]
    fn test_filter_by_glob_pattern() {
        let root = PathBuf::from("/project");
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("tests/test.rs"),
        ];

        let file_set = FileSet::from_files(root, files);
        let filtered = file_set.filter_by_glob("**/src/*.rs");

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_by_glob_wildcard() {
        let root = PathBuf::from("/project");
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/main.go"),
            PathBuf::from("src/main.py"),
        ];

        let file_set = FileSet::from_files(root, files);
        let filtered = file_set.filter_by_glob("*.rs");

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_by_glob_invalid_pattern() {
        let root = PathBuf::from("/project");
        let files = vec![PathBuf::from("src/main.rs")];

        let file_set = FileSet::from_files(root, files);
        // Invalid glob should return original set
        let filtered = file_set.filter_by_glob("[invalid");

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_exclude_by_glob_pattern() {
        let root = PathBuf::from("/project");
        let files = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("tests/test.rs"),
        ];

        let file_set = FileSet::from_files(root, files);
        let filtered = file_set.exclude_by_glob("**/tests/*.rs");

        assert_eq!(filtered.len(), 2);
        assert!(!filtered
            .files()
            .iter()
            .any(|f| f.to_string_lossy().contains("test.rs")));
    }

    #[test]
    fn test_exclude_by_glob_filename() {
        let root = PathBuf::from("/project");
        let files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];

        let file_set = FileSet::from_files(root, files);
        let filtered = file_set.exclude_by_glob("main.rs");

        assert_eq!(filtered.len(), 1);
        assert!(filtered.files()[0].to_string_lossy().contains("lib.rs"));
    }
}
