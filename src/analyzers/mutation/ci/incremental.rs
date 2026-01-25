//! Incremental mutation testing for CI/CD.
//!
//! Filters mutants to only those in files changed since a base reference.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::{Error, Result};

use super::super::Mutant;

/// Incremental mutation testing filter.
///
/// Filters mutants to only those affecting files that have changed
/// since a base git reference (branch, tag, or commit).
pub struct IncrementalMutation {
    /// Base reference to compare against (e.g., "main", "origin/main").
    base_ref: String,
}

impl IncrementalMutation {
    /// Create a new incremental mutation filter.
    pub fn new(base_ref: &str) -> Self {
        Self {
            base_ref: base_ref.to_string(),
        }
    }

    /// Get the base reference.
    pub fn base_ref(&self) -> &str {
        &self.base_ref
    }

    /// Get files changed since the base reference using git diff.
    ///
    /// Returns absolute paths to changed files.
    pub fn get_changed_files(&self, repo_path: &Path) -> Result<Vec<PathBuf>> {
        // Check if we're in a git repository
        if !repo_path.join(".git").exists() && self.find_git_root(repo_path).is_none() {
            return Err(Error::git("Not a git repository"));
        }

        let output = Command::new("git")
            .args(["diff", "--name-only", &self.base_ref])
            .current_dir(repo_path)
            .output()
            .map_err(|e| Error::git(format!("Failed to run git diff: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Handle the case where base_ref doesn't exist
            if stderr.contains("unknown revision") || stderr.contains("bad revision") {
                return Err(Error::git(format!(
                    "Unknown reference '{}': {}",
                    self.base_ref, stderr
                )));
            }
            return Err(Error::git(format!("git diff failed: {}", stderr)));
        }

        let files = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| repo_path.join(line))
            .collect();

        Ok(files)
    }

    /// Filter mutants to only those in changed files.
    pub fn filter_to_changes(&self, mutants: Vec<Mutant>, repo_path: &Path) -> Result<Vec<Mutant>> {
        let changed_files: HashSet<PathBuf> = self
            .get_changed_files(repo_path)?
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();

        let filtered = mutants
            .into_iter()
            .filter(|m| {
                m.file_path
                    .canonicalize()
                    .map(|p| changed_files.contains(&p))
                    .unwrap_or(false)
            })
            .collect();

        Ok(filtered)
    }

    /// Get changed line ranges for a specific file.
    ///
    /// Returns a list of (start_line, end_line) tuples representing
    /// the ranges of lines that were changed.
    pub fn get_changed_lines(&self, file: &Path, repo_path: &Path) -> Result<Vec<(u32, u32)>> {
        // Get the relative path from repo root
        let relative_path = file
            .strip_prefix(repo_path)
            .unwrap_or(file)
            .to_string_lossy();

        let output = Command::new("git")
            .args(["diff", "-U0", &self.base_ref, "--", relative_path.as_ref()])
            .current_dir(repo_path)
            .output()
            .map_err(|e| Error::git(format!("Failed to run git diff: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::git(format!("git diff failed: {}", stderr)));
        }

        let diff_output = String::from_utf8_lossy(&output.stdout);
        let ranges = parse_diff_ranges(&diff_output);

        Ok(ranges)
    }

    /// Filter mutants to only those on changed lines.
    pub fn filter_to_changed_lines(
        &self,
        mutants: Vec<Mutant>,
        repo_path: &Path,
    ) -> Result<Vec<Mutant>> {
        // Group mutants by file
        let mut by_file: std::collections::HashMap<PathBuf, Vec<Mutant>> =
            std::collections::HashMap::new();
        for mutant in mutants {
            by_file
                .entry(mutant.file_path.clone())
                .or_default()
                .push(mutant);
        }

        let mut filtered = Vec::new();

        for (file, file_mutants) in by_file {
            let ranges = match self.get_changed_lines(&file, repo_path) {
                Ok(r) => r,
                Err(_) => continue, // Skip files we can't get diff info for
            };

            for mutant in file_mutants {
                if ranges
                    .iter()
                    .any(|&(start, end)| mutant.line >= start && mutant.line <= end)
                {
                    filtered.push(mutant);
                }
            }
        }

        Ok(filtered)
    }

    /// Find the git root directory from a given path.
    fn find_git_root(&self, path: &Path) -> Option<PathBuf> {
        let mut current = path.to_path_buf();
        loop {
            if current.join(".git").exists() {
                return Some(current);
            }
            if !current.pop() {
                return None;
            }
        }
    }
}

/// Parse git diff output to extract changed line ranges.
///
/// Parses the @@ hunk headers to extract the line ranges.
fn parse_diff_ranges(diff_output: &str) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();

    for line in diff_output.lines() {
        if line.starts_with("@@") {
            if let Some(range) = parse_hunk_header(line) {
                ranges.push(range);
            }
        }
    }

    ranges
}

/// Parse a single hunk header like "@@ -1,3 +1,4 @@" to extract the new line range.
fn parse_hunk_header(header: &str) -> Option<(u32, u32)> {
    // Find the +start,count or +start portion
    for part in header.split_whitespace() {
        if let Some(range_str) = part.strip_prefix('+') {
            let (start, count) = if let Some(comma_pos) = range_str.find(',') {
                let start: u32 = range_str[..comma_pos].parse().ok()?;
                let count: u32 = range_str[comma_pos + 1..].parse().ok()?;
                (start, count)
            } else {
                let start: u32 = range_str.parse().ok()?;
                (start, 1)
            };

            // Handle the case where count is 0 (deletion only, no new lines)
            if count == 0 {
                return None;
            }

            return Some((start, start + count - 1));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn init_git_repo(path: &Path) {
        StdCommand::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .expect("failed to init git repo");
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .expect("failed to set git email");
        StdCommand::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .expect("failed to set git name");
    }

    fn make_commit(path: &Path, message: &str) {
        StdCommand::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .expect("failed to add files");
        StdCommand::new("git")
            .args(["commit", "-m", message, "--allow-empty"])
            .current_dir(path)
            .output()
            .expect("failed to commit");
    }

    #[test]
    fn test_incremental_new() {
        let inc = IncrementalMutation::new("main");
        assert_eq!(inc.base_ref(), "main");
    }

    #[test]
    fn test_get_changed_files_not_git_repo() {
        let temp = tempfile::tempdir().unwrap();
        let inc = IncrementalMutation::new("main");

        let result = inc.get_changed_files(temp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_get_changed_files_invalid_ref() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        std::fs::write(temp.path().join("file.rs"), "fn main() {}").unwrap();
        make_commit(temp.path(), "initial");

        let inc = IncrementalMutation::new("nonexistent-branch");
        let result = inc.get_changed_files(temp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_get_changed_files_with_changes() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        // Create initial commit
        std::fs::write(temp.path().join("file.rs"), "fn main() {}").unwrap();
        make_commit(temp.path(), "initial");

        // Create a branch to compare against
        StdCommand::new("git")
            .args(["branch", "base"])
            .current_dir(temp.path())
            .output()
            .expect("failed to create branch");

        // Make a change
        std::fs::write(
            temp.path().join("file.rs"),
            "fn main() { println!(\"hello\"); }",
        )
        .unwrap();

        let inc = IncrementalMutation::new("base");
        let result = inc.get_changed_files(temp.path()).unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("file.rs"));
    }

    #[test]
    fn test_get_changed_files_no_changes() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        std::fs::write(temp.path().join("file.rs"), "fn main() {}").unwrap();
        make_commit(temp.path(), "initial");

        let inc = IncrementalMutation::new("HEAD");
        let result = inc.get_changed_files(temp.path()).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_to_changes() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        // Create two files
        std::fs::write(temp.path().join("changed.rs"), "fn a() {}").unwrap();
        std::fs::write(temp.path().join("unchanged.rs"), "fn b() {}").unwrap();
        make_commit(temp.path(), "initial");

        // Create base branch
        StdCommand::new("git")
            .args(["branch", "base"])
            .current_dir(temp.path())
            .output()
            .unwrap();

        // Modify only one file
        std::fs::write(temp.path().join("changed.rs"), "fn a() { changed }").unwrap();

        let mutants = vec![
            Mutant::new(
                "1",
                temp.path().join("changed.rs"),
                "CRR",
                1,
                1,
                "1",
                "0",
                "desc",
                (0, 1),
            ),
            Mutant::new(
                "2",
                temp.path().join("unchanged.rs"),
                "CRR",
                1,
                1,
                "1",
                "0",
                "desc",
                (0, 1),
            ),
        ];

        let inc = IncrementalMutation::new("base");
        let filtered = inc.filter_to_changes(mutants, temp.path()).unwrap();

        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].file_path.ends_with("changed.rs"));
    }

    #[test]
    fn test_parse_hunk_header_simple() {
        let result = parse_hunk_header("@@ -1 +1 @@");
        assert_eq!(result, Some((1, 1)));
    }

    #[test]
    fn test_parse_hunk_header_with_count() {
        let result = parse_hunk_header("@@ -1,3 +1,5 @@");
        assert_eq!(result, Some((1, 5)));
    }

    #[test]
    fn test_parse_hunk_header_deletion_only() {
        // When count is 0, no new lines were added
        let result = parse_hunk_header("@@ -1,3 +1,0 @@");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_diff_ranges() {
        let diff = r#"diff --git a/file.rs b/file.rs
index abc123..def456 100644
--- a/file.rs
+++ b/file.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!("hello");
 }
@@ -10,2 +11,5 @@
 fn other() {
"#;
        let ranges = parse_diff_ranges(diff);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (1, 4));
        assert_eq!(ranges[1], (11, 15));
    }

    #[test]
    fn test_get_changed_lines() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        // Create initial file
        std::fs::write(
            temp.path().join("file.rs"),
            "line1\nline2\nline3\nline4\nline5\n",
        )
        .unwrap();
        make_commit(temp.path(), "initial");

        // Create base branch
        StdCommand::new("git")
            .args(["branch", "base"])
            .current_dir(temp.path())
            .output()
            .unwrap();

        // Modify line 3
        std::fs::write(
            temp.path().join("file.rs"),
            "line1\nline2\nmodified\nline4\nline5\n",
        )
        .unwrap();

        let inc = IncrementalMutation::new("base");
        let ranges = inc
            .get_changed_lines(&temp.path().join("file.rs"), temp.path())
            .unwrap();

        // Should have at least one range that includes line 3
        assert!(!ranges.is_empty());
    }

    #[test]
    fn test_filter_to_changed_lines() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        // Create initial file with 5 lines
        std::fs::write(
            temp.path().join("file.rs"),
            "line1\nline2\nline3\nline4\nline5\n",
        )
        .unwrap();
        make_commit(temp.path(), "initial");

        // Create base branch
        StdCommand::new("git")
            .args(["branch", "base"])
            .current_dir(temp.path())
            .output()
            .unwrap();

        // Modify line 3
        std::fs::write(
            temp.path().join("file.rs"),
            "line1\nline2\nmodified3\nline4\nline5\n",
        )
        .unwrap();

        let mutants = vec![
            Mutant::new(
                "1",
                temp.path().join("file.rs"),
                "CRR",
                1,
                1,
                "1",
                "0",
                "line 1",
                (0, 5),
            ),
            Mutant::new(
                "2",
                temp.path().join("file.rs"),
                "CRR",
                3,
                1,
                "1",
                "0",
                "line 3",
                (12, 17),
            ),
            Mutant::new(
                "3",
                temp.path().join("file.rs"),
                "CRR",
                5,
                1,
                "1",
                "0",
                "line 5",
                (30, 35),
            ),
        ];

        let inc = IncrementalMutation::new("base");
        let filtered = inc.filter_to_changed_lines(mutants, temp.path()).unwrap();

        // Only mutant on line 3 should be included
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].line, 3);
    }

    #[test]
    fn test_find_git_root() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        let subdir = temp.path().join("src").join("module");
        std::fs::create_dir_all(&subdir).unwrap();

        let inc = IncrementalMutation::new("main");
        let root = inc.find_git_root(&subdir);

        assert!(root.is_some());
        assert_eq!(root.unwrap(), temp.path());
    }

    #[test]
    fn test_find_git_root_not_found() {
        let temp = tempfile::tempdir().unwrap();
        // Don't init git

        let inc = IncrementalMutation::new("main");
        let root = inc.find_git_root(temp.path());

        assert!(root.is_none());
    }
}
