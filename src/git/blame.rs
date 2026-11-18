//! Git blame operations.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use gix::Repository;
use serde::{Deserialize, Serialize};

use crate::core::{Error, Result};

/// Blame information for a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlameInfo {
    /// File path.
    pub path: String,
    /// Total number of blamed lines.
    pub total_lines: u32,
    /// Aggregated author statistics. A line of a boundary commit has no author here.
    pub authors: HashMap<String, AuthorStats>,
    /// The lines that git gives to a boundary commit of a shallow clone. Git
    /// does not have the history before that commit, thus the author of such
    /// a line is not known. The percentages do not include these lines.
    #[serde(default)]
    pub boundary_lines: u32,
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
pub fn get_blame(
    _repo: &Repository,
    root: &Path,
    path: &Path,
    shallow_boundary: &HashSet<String>,
) -> Result<BlameInfo> {
    let relative_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();

    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(["blame", "--line-porcelain", "--", &relative_path])
        .output()
        .map_err(|e| Error::git(format!("Failed to run git blame: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::git(format!("git blame failed: {}", stderr)));
    }

    parse_line_porcelain(&output.stdout, path, shallow_boundary)
}

/// The commit hash at the start of the header of a blame entry.
fn entry_commit(line: &str) -> Option<&str> {
    let hash = line.split(' ').next()?;
    let is_hash =
        matches!(hash.len(), 40 | 64) && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
    is_hash.then_some(hash)
}

/// Parse `git blame --line-porcelain` output into BlameInfo.
///
/// `shallow_boundary` has the hashes of the commits at which a shallow clone
/// stops. Git gives each line that is older than such a commit to that
/// commit, thus its author is not the author of the line.
fn parse_line_porcelain(
    output: &[u8],
    path: &Path,
    shallow_boundary: &HashSet<String>,
) -> Result<BlameInfo> {
    let text = String::from_utf8_lossy(output);

    let mut total_lines = 0u32;
    let mut boundary_lines = 0u32;
    let mut authors: HashMap<String, AuthorStats> = HashMap::new();
    let mut current_author = "";
    let mut current_timestamp: i64 = 0;
    let mut current_is_boundary = false;

    for line in text.lines() {
        if line.starts_with('\t') {
            // Content line - marks end of a blame entry
            total_lines += 1;
            if current_is_boundary {
                boundary_lines += 1;
            } else if let Some(stats) = authors.get_mut(current_author) {
                stats.lines += 1;
                stats.first_commit = stats.first_commit.min(current_timestamp);
                stats.last_commit = stats.last_commit.max(current_timestamp);
            } else {
                authors.insert(
                    current_author.to_string(),
                    AuthorStats {
                        lines: 1,
                        percentage: 0.0,
                        first_commit: current_timestamp,
                        last_commit: current_timestamp,
                    },
                );
            }
        } else if let Some(rest) = line.strip_prefix("author ") {
            current_author = rest;
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            current_timestamp = rest.parse().unwrap_or(0);
        } else if let Some(commit) = entry_commit(line) {
            current_is_boundary = shallow_boundary.contains(commit);
        }
    }

    let attributed_lines = total_lines - boundary_lines;
    for stats in authors.values_mut() {
        stats.percentage = if attributed_lines > 0 {
            (f64::from(stats.lines) / f64::from(attributed_lines)) * 100.0
        } else {
            0.0
        };
    }

    Ok(BlameInfo {
        path: path.to_string_lossy().to_string(),
        total_lines,
        authors,
        boundary_lines,
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

    const PORCELAIN: &str = "1111111111111111111111111111111111111111 1 1 2\nauthor Ada\nauthor-time 100\nsummary first\nfilename a.rs\n\tline one\n1111111111111111111111111111111111111111 2 2\nauthor Ada\nauthor-time 100\nsummary first\nfilename a.rs\n\tline two\n2222222222222222222222222222222222222222 3 3 1\nauthor Grace\nauthor-time 200\nsummary second\nfilename a.rs\n\tline three\n";

    #[test]
    fn test_porcelain_without_a_boundary_gives_each_line_to_its_author() {
        let info =
            parse_line_porcelain(PORCELAIN.as_bytes(), Path::new("a.rs"), &HashSet::new()).unwrap();
        assert_eq!(info.total_lines, 3);
        assert_eq!(info.boundary_lines, 0);
        assert_eq!(info.authors["Ada"].lines, 2);
        assert_eq!(info.authors["Grace"].lines, 1);
    }

    #[test]
    fn test_lines_of_a_shallow_boundary_commit_have_no_author() {
        let boundary = HashSet::from(["1111111111111111111111111111111111111111".to_string()]);
        let info =
            parse_line_porcelain(PORCELAIN.as_bytes(), Path::new("a.rs"), &boundary).unwrap();
        assert_eq!(info.total_lines, 3);
        assert_eq!(info.boundary_lines, 2);
        assert!(!info.authors.contains_key("Ada"));
        assert_eq!(info.authors["Grace"].lines, 1);
        assert!((info.authors["Grace"].percentage - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_a_content_line_that_looks_like_a_hash_is_not_a_header() {
        let text = "2222222222222222222222222222222222222222 1 1 1\nauthor Grace\nauthor-time 200\nfilename a.rs\n\t1111111111111111111111111111111111111111 is a hash in the source\n";
        let boundary = HashSet::from(["1111111111111111111111111111111111111111".to_string()]);
        let info = parse_line_porcelain(text.as_bytes(), Path::new("a.rs"), &boundary).unwrap();
        assert_eq!(info.boundary_lines, 0);
        assert_eq!(info.authors["Grace"].lines, 1);
    }

    #[test]
    fn test_bus_factor_empty() {
        let info = BlameInfo {
            path: "test.rs".to_string(),
            total_lines: 0,
            authors: HashMap::new(),
            boundary_lines: 0,
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
            total_lines: 100,
            authors,
            boundary_lines: 0,
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
            total_lines: 100,
            authors,
            boundary_lines: 0,
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
            total_lines: 80,
            authors,
            boundary_lines: 0,
        };

        assert!((info.ownership_ratio() - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_ownership_ratio_empty() {
        let info = BlameInfo {
            path: "test.rs".to_string(),
            total_lines: 0,
            authors: HashMap::new(),
            boundary_lines: 0,
        };

        assert!((info.ownership_ratio()).abs() < 0.001);
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
        let result = get_blame(&repo, temp.path(), &file_path, &HashSet::new());

        assert!(result.is_ok());
        let blame = result.unwrap();

        // Verify blame results
        assert_eq!(blame.total_lines, 3); // 3 lines in the file
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
        let result = get_blame(&repo, temp.path(), &file_path, &HashSet::new());

        assert!(result.is_ok());
        let blame = result.unwrap();

        // Verify we have at least one author
        assert!(!blame.authors.is_empty());
        assert_eq!(blame.total_lines, 4); // 4 lines in the updated file
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
            total_lines: 50,
            authors,
            boundary_lines: 0,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"path\":\"test.rs\""));
        assert!(json.contains("\"total_lines\":50"));
        assert!(json.contains("\"lines\":50"));
    }
}
