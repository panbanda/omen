//! Git log operations.

use std::path::PathBuf;

use gix::Repository;
use serde::{Deserialize, Serialize};

use crate::core::{Error, Result};

/// A git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// Commit SHA.
    pub sha: String,
    /// Author name.
    pub author: String,
    /// Author email.
    pub email: String,
    /// Commit timestamp.
    pub timestamp: i64,
    /// Commit message (first line).
    pub message: String,
    /// Files changed in this commit.
    pub files: Vec<FileChange>,
}

/// A file change in a commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    /// File path.
    pub path: PathBuf,
    /// Lines added.
    pub additions: u32,
    /// Lines deleted.
    pub deletions: u32,
    /// Change type (added, modified, deleted, renamed).
    pub change_type: ChangeType,
}

/// Type of file change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// Statistics for a single commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitStats {
    /// Commit SHA.
    pub sha: String,
    /// Total lines added.
    pub additions: u32,
    /// Total lines deleted.
    pub deletions: u32,
    /// Number of files changed.
    pub files_changed: usize,
}

/// Get commit log from repository.
pub fn get_log(
    _repo: &Repository,
    _since: Option<&str>,
    _paths: Option<&[PathBuf]>,
) -> Result<Vec<Commit>> {
    // TODO: Implement using gix
    // This is a placeholder that returns empty results
    // Full implementation would use repo.rev_walk() and iterate commits
    Ok(Vec::new())
}

/// Get statistics for a specific commit.
pub fn get_commit_stats(_repo: &Repository, _sha: &str) -> Result<CommitStats> {
    // TODO: Implement using gix
    Err(Error::git("Not implemented"))
}

/// Get diff stats between two refs.
pub fn get_diff_stats(_repo: &Repository, _from: &str, _to: &str) -> Result<Vec<FileChange>> {
    // TODO: Implement using gix
    Ok(Vec::new())
}

/// Get merge base between two refs.
pub fn get_merge_base(_repo: &Repository, _ref1: &str, _ref2: &str) -> Result<String> {
    // TODO: Implement using gix
    Err(Error::git("Not implemented"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_struct() {
        let commit = Commit {
            sha: "abc123".to_string(),
            author: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            timestamp: 1704067200,
            message: "Initial commit".to_string(),
            files: vec![],
        };
        assert_eq!(commit.sha, "abc123");
        assert_eq!(commit.author, "Test Author");
        assert_eq!(commit.email, "test@example.com");
        assert_eq!(commit.timestamp, 1704067200);
        assert_eq!(commit.message, "Initial commit");
        assert!(commit.files.is_empty());
    }

    #[test]
    fn test_file_change_struct() {
        let change = FileChange {
            path: PathBuf::from("src/main.rs"),
            additions: 10,
            deletions: 5,
            change_type: ChangeType::Modified,
        };
        assert_eq!(change.path, PathBuf::from("src/main.rs"));
        assert_eq!(change.additions, 10);
        assert_eq!(change.deletions, 5);
        assert_eq!(change.change_type, ChangeType::Modified);
    }

    #[test]
    fn test_change_type_variants() {
        assert_eq!(ChangeType::Added, ChangeType::Added);
        assert_eq!(ChangeType::Modified, ChangeType::Modified);
        assert_eq!(ChangeType::Deleted, ChangeType::Deleted);
        assert_eq!(ChangeType::Renamed, ChangeType::Renamed);
    }

    #[test]
    fn test_commit_stats_struct() {
        let stats = CommitStats {
            sha: "abc123".to_string(),
            additions: 100,
            deletions: 50,
            files_changed: 5,
        };
        assert_eq!(stats.sha, "abc123");
        assert_eq!(stats.additions, 100);
        assert_eq!(stats.deletions, 50);
        assert_eq!(stats.files_changed, 5);
    }

    #[test]
    fn test_commit_serialization() {
        let commit = Commit {
            sha: "abc123".to_string(),
            author: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            timestamp: 1704067200,
            message: "Test commit".to_string(),
            files: vec![],
        };
        let json = serde_json::to_string(&commit).unwrap();
        assert!(json.contains("\"sha\":\"abc123\""));
        assert!(json.contains("\"author\":\"Test Author\""));
    }

    #[test]
    fn test_file_change_serialization() {
        let change = FileChange {
            path: PathBuf::from("test.rs"),
            additions: 10,
            deletions: 5,
            change_type: ChangeType::Added,
        };
        let json = serde_json::to_string(&change).unwrap();
        assert!(json.contains("\"additions\":10"));
        assert!(json.contains("\"deletions\":5"));
        assert!(json.contains("\"change_type\":\"added\""));
    }

    #[test]
    fn test_commit_stats_serialization() {
        let stats = CommitStats {
            sha: "abc123".to_string(),
            additions: 100,
            deletions: 50,
            files_changed: 5,
        };
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("\"additions\":100"));
        assert!(json.contains("\"deletions\":50"));
        assert!(json.contains("\"files_changed\":5"));
    }

    #[test]
    fn test_commit_with_files() {
        let commit = Commit {
            sha: "abc123".to_string(),
            author: "Test Author".to_string(),
            email: "test@example.com".to_string(),
            timestamp: 1704067200,
            message: "Test commit".to_string(),
            files: vec![
                FileChange {
                    path: PathBuf::from("file1.rs"),
                    additions: 10,
                    deletions: 5,
                    change_type: ChangeType::Modified,
                },
                FileChange {
                    path: PathBuf::from("file2.rs"),
                    additions: 20,
                    deletions: 0,
                    change_type: ChangeType::Added,
                },
            ],
        };
        assert_eq!(commit.files.len(), 2);
        assert_eq!(commit.files[0].path, PathBuf::from("file1.rs"));
        assert_eq!(commit.files[1].path, PathBuf::from("file2.rs"));
    }

    #[test]
    fn test_change_type_serialization() {
        assert_eq!(
            serde_json::to_string(&ChangeType::Added).unwrap(),
            "\"added\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeType::Modified).unwrap(),
            "\"modified\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeType::Deleted).unwrap(),
            "\"deleted\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeType::Renamed).unwrap(),
            "\"renamed\""
        );
    }

    #[test]
    fn test_change_type_deserialization() {
        assert_eq!(
            serde_json::from_str::<ChangeType>("\"added\"").unwrap(),
            ChangeType::Added
        );
        assert_eq!(
            serde_json::from_str::<ChangeType>("\"modified\"").unwrap(),
            ChangeType::Modified
        );
        assert_eq!(
            serde_json::from_str::<ChangeType>("\"deleted\"").unwrap(),
            ChangeType::Deleted
        );
        assert_eq!(
            serde_json::from_str::<ChangeType>("\"renamed\"").unwrap(),
            ChangeType::Renamed
        );
    }
}
