//! Git blame operations.

use std::collections::HashMap;
use std::path::Path;

use gix::Repository;
use serde::{Deserialize, Serialize};

use crate::core::{Error, Result};

/// Blame information for a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameInfo {
    /// File path.
    pub path: String,
    /// Lines with their authors.
    pub lines: Vec<BlameLine>,
    /// Aggregated author statistics.
    pub authors: HashMap<String, AuthorStats>,
}

/// A single line with blame information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameLine {
    /// Line number (1-indexed).
    pub line: u32,
    /// Author name.
    pub author: String,
    /// Commit SHA.
    pub sha: String,
    /// Commit timestamp.
    pub timestamp: i64,
}

/// Statistics for an author.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthorStats {
    /// Number of lines owned.
    pub lines: u32,
    /// Percentage of file owned.
    pub percentage: f64,
    /// First contribution timestamp.
    pub first_commit: i64,
    /// Last contribution timestamp.
    pub last_commit: i64,
}

/// Get blame information for a file using git CLI (fast path).
///
/// Uses `git blame --line-porcelain` which is much faster than gix's pure-Rust
/// blame implementation, especially on large repositories with deep history.
pub fn get_blame(_repo: &Repository, root: &Path, path: &Path) -> Result<BlameInfo> {
    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["blame", "--line-porcelain", &relative_path])
        .output()
        .map_err(|e| Error::git(format!("Failed to run git blame: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::git(format!("git blame failed: {}", stderr)));
    }

    parse_line_porcelain(&output.stdout, path)
}

/// Parse `git blame --line-porcelain` output into BlameInfo.
fn parse_line_porcelain(output: &[u8], path: &Path) -> Result<BlameInfo> {
    let text = String::from_utf8_lossy(output);

    let mut lines = Vec::new();
    let mut author_lines: HashMap<String, Vec<i64>> = HashMap::new();

    let mut current_sha = String::new();
    let mut current_author = String::new();
    let mut current_timestamp: i64 = 0;
    let mut current_line_num: u32 = 0;

    for line in text.lines() {
        if line.starts_with('\t') {
            // Content line - marks end of a blame entry
            lines.push(BlameLine {
                line: current_line_num,
                author: current_author.clone(),
                sha: current_sha.clone(),
                timestamp: current_timestamp,
            });
            author_lines
                .entry(current_author.clone())
                .or_default()
                .push(current_timestamp);
        } else if let Some(rest) = line.strip_prefix("author ") {
            current_author = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            current_timestamp = rest.parse().unwrap_or(0);
        } else if line.len() >= 40 && line.as_bytes()[0].is_ascii_hexdigit() {
            // Header line: <sha> <orig-line> <final-line> [<num-lines>]
            let parts: Vec<&str> = line.splitn(4, ' ').collect();
            if parts.len() >= 3 {
                current_sha = parts[0].to_string();
                current_line_num = parts[2].parse().unwrap_or(0);
            }
        }
    }

    let total_lines = lines.len() as f64;
    let mut authors = HashMap::new();

    for (name, timestamps) in author_lines {
        let line_count = timestamps.len() as u32;
        let percentage = if total_lines > 0.0 {
            (line_count as f64 / total_lines) * 100.0
        } else {
            0.0
        };

        let first_commit = timestamps.iter().min().copied().unwrap_or(0);
        let last_commit = timestamps.iter().max().copied().unwrap_or(0);

        authors.insert(
            name,
            AuthorStats {
                lines: line_count,
                percentage,
                first_commit,
                last_commit,
            },
        );
    }

    Ok(BlameInfo {
        path: path.to_string_lossy().to_string(),
        lines,
        authors,
    })
}

impl BlameInfo {
    /// Calculate the bus factor (number of significant contributors).
    pub fn bus_factor(&self) -> usize {
        self.authors
            .values()
            .filter(|stats| stats.percentage > 5.0)
            .count()
    }

    /// Get the primary owner (author with most lines).
    pub fn primary_owner(&self) -> Option<(&str, f64)> {
        self.authors
            .iter()
            .max_by(|a, b| a.1.lines.cmp(&b.1.lines))
            .map(|(name, stats)| (name.as_str(), stats.percentage))
    }

    /// Calculate ownership concentration.
    pub fn ownership_ratio(&self) -> f64 {
        self.primary_owner()
            .map(|(_, pct)| pct / 100.0)
            .unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_bus_factor_empty() {
        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors: HashMap::new(),
        };
        assert_eq!(info.bus_factor(), 0);
    }

    #[test]
    fn test_bus_factor_with_authors() {
        let mut authors = HashMap::new();
        authors.insert(
            "Alice".to_string(),
            AuthorStats {
                lines: 80,
                percentage: 80.0,
                first_commit: 0,
                last_commit: 0,
            },
        );
        authors.insert(
            "Bob".to_string(),
            AuthorStats {
                lines: 15,
                percentage: 15.0,
                first_commit: 0,
                last_commit: 0,
            },
        );
        authors.insert(
            "Carol".to_string(),
            AuthorStats {
                lines: 5,
                percentage: 5.0,
                first_commit: 0,
                last_commit: 0,
            },
        );

        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors,
        };

        assert_eq!(info.bus_factor(), 2); // Alice and Bob have >5%
    }

    #[test]
    fn test_primary_owner() {
        let mut authors = HashMap::new();
        authors.insert(
            "Alice".to_string(),
            AuthorStats {
                lines: 70,
                percentage: 70.0,
                first_commit: 100,
                last_commit: 200,
            },
        );
        authors.insert(
            "Bob".to_string(),
            AuthorStats {
                lines: 30,
                percentage: 30.0,
                first_commit: 150,
                last_commit: 250,
            },
        );

        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors,
        };

        let (owner, pct) = info.primary_owner().unwrap();
        assert_eq!(owner, "Alice");
        assert!((pct - 70.0).abs() < 0.001);
    }

    #[test]
    fn test_ownership_ratio() {
        let mut authors = HashMap::new();
        authors.insert(
            "Alice".to_string(),
            AuthorStats {
                lines: 80,
                percentage: 80.0,
                first_commit: 0,
                last_commit: 0,
            },
        );

        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors,
        };

        assert!((info.ownership_ratio() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_ownership_ratio_empty() {
        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: Vec::new(),
            authors: HashMap::new(),
        };

        assert!((info.ownership_ratio()).abs() < 0.001);
    }

    #[test]
    fn test_blame_line_struct() {
        let line = BlameLine {
            line: 42,
            author: "Alice".to_string(),
            sha: "abc123def456".to_string(),
            timestamp: 1700000000,
        };
        assert_eq!(line.line, 42);
        assert_eq!(line.author, "Alice");
        assert_eq!(line.sha, "abc123def456");
        assert_eq!(line.timestamp, 1700000000);
    }

    #[test]
    fn test_author_stats_default() {
        let stats = AuthorStats::default();
        assert_eq!(stats.lines, 0);
        assert!((stats.percentage).abs() < 0.001);
        assert_eq!(stats.first_commit, 0);
        assert_eq!(stats.last_commit, 0);
    }

    fn init_test_repo(path: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .expect("failed to init git repo");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .expect("failed to set git email");
        Command::new("git")
            .args(["config", "user.name", "Test Author"])
            .current_dir(path)
            .output()
            .expect("failed to set git name");
    }

    #[test]
    fn test_get_blame_with_real_repo() {
        let temp = tempfile::tempdir().unwrap();
        init_test_repo(temp.path());

        // Create and commit a file
        let file_path = temp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp.path())
            .output()
            .expect("failed to add file");
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(temp.path())
            .output()
            .expect("failed to commit");

        // Open repo and get blame
        let repo = gix::open(temp.path()).unwrap();
        let result = get_blame(&repo, temp.path(), &file_path);

        assert!(result.is_ok());
        let blame = result.unwrap();

        // Verify blame results
        assert_eq!(blame.lines.len(), 3); // 3 lines in the file
        assert_eq!(blame.authors.len(), 1); // 1 author
        assert!(blame.authors.contains_key("Test Author"));

        let author_stats = &blame.authors["Test Author"];
        assert_eq!(author_stats.lines, 3);
        assert!((author_stats.percentage - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_get_blame_multiple_authors() {
        let temp = tempfile::tempdir().unwrap();
        init_test_repo(temp.path());

        // First author commits
        let file_path = temp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {\n    println!(\"hello\");\n}\n").unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp.path())
            .output()
            .expect("failed to add file");
        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(temp.path())
            .output()
            .expect("failed to commit");

        // Change author and add more lines
        Command::new("git")
            .args(["config", "user.name", "Second Author"])
            .current_dir(temp.path())
            .output()
            .expect("failed to set git name");
        std::fs::write(
            &file_path,
            "fn main() {\n    println!(\"hello\");\n    println!(\"world\");\n}\n",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp.path())
            .output()
            .expect("failed to add file");
        Command::new("git")
            .args(["commit", "-m", "Add world"])
            .current_dir(temp.path())
            .output()
            .expect("failed to commit");

        // Open repo and get blame
        let repo = gix::open(temp.path()).unwrap();
        let result = get_blame(&repo, temp.path(), &file_path);

        assert!(result.is_ok());
        let blame = result.unwrap();

        // Verify we have at least one author
        assert!(!blame.authors.is_empty());
        assert_eq!(blame.lines.len(), 4); // 4 lines in the updated file
    }

    #[test]
    fn test_blame_info_serialization() {
        let mut authors = HashMap::new();
        authors.insert(
            "Alice".to_string(),
            AuthorStats {
                lines: 50,
                percentage: 50.0,
                first_commit: 1000,
                last_commit: 2000,
            },
        );

        let info = BlameInfo {
            path: "test.rs".to_string(),
            lines: vec![BlameLine {
                line: 1,
                author: "Alice".to_string(),
                sha: "abc123".to_string(),
                timestamp: 1500,
            }],
            authors,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"path\":\"test.rs\""));
        assert!(json.contains("\"author\":\"Alice\""));
        assert!(json.contains("\"lines\":50"));
    }
}
