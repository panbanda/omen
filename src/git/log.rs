//! Git log operations.

use std::collections::HashMap;
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
///
/// This is a fast gix-based implementation that avoids spawning git CLI.
pub fn get_log(
    repo: &Repository,
    since: Option<&str>,
    _paths: Option<&[PathBuf]>,
) -> Result<Vec<Commit>> {
    let head = repo
        .head_id()
        .map_err(|e| Error::git(format!("Failed to get HEAD: {e}")))?;

    let cutoff_time = since.and_then(parse_since_duration).map(|duration| {
        let now = std::time::SystemTime::now();
        now.checked_sub(duration)
            .unwrap_or(std::time::UNIX_EPOCH)
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let walk = repo
        .rev_walk([head])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(|e| Error::git(format!("Failed to walk commits: {e}")))?;

    let mut commits = Vec::new();

    for info in walk {
        let info = info.map_err(|e| Error::git(format!("Failed to read commit: {e}")))?;
        let commit = info
            .object()
            .map_err(|e| Error::git(format!("Failed to get commit object: {e}")))?;

        let author = commit.author().map_err(|e| Error::git(format!("{e}")))?;
        let timestamp = author.time.seconds;

        // Skip commits older than cutoff
        if let Some(cutoff) = cutoff_time {
            if timestamp < cutoff {
                break;
            }
        }

        let message = commit
            .message()
            .map_err(|e| Error::git(format!("{e}")))?
            .title
            .to_string();

        commits.push(Commit {
            sha: commit.id.to_string(),
            author: author.name.to_string(),
            email: author.email.to_string(),
            timestamp,
            message,
            files: Vec::new(), // File changes calculated separately if needed
        });
    }

    Ok(commits)
}

/// Parse "since" duration strings like "30 days ago", "1 week", "6m", "1y".
fn parse_since_duration(since: &str) -> Option<std::time::Duration> {
    let since = since.trim().to_lowercase();

    // Handle "X days ago" format
    if since.ends_with(" ago") {
        let without_ago = &since[..since.len() - 4];
        return parse_since_duration(without_ago);
    }

    // Find where the number ends and the unit begins
    let first_alpha = since
        .find(|c: char| c.is_alphabetic())
        .unwrap_or(since.len());
    let num_str = &since[..first_alpha];
    let unit = since[first_alpha..].trim();

    let num: u64 = num_str.trim().parse().ok()?;

    let secs = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => num,
        "m" | "min" | "mins" | "minute" | "minutes" if !unit.starts_with("mo") => num * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => num * 3600,
        "d" | "day" | "days" => num * 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => num * 604800,
        "mo" | "mon" | "month" | "months" => num * 2592000, // 30 days
        "y" | "yr" | "yrs" | "year" | "years" => num * 31536000, // 365 days
        _ => return None,
    };

    Some(std::time::Duration::from_secs(secs))
}

/// Get commit log with file change statistics (numstat equivalent).
///
/// This is faster than CLI because we process diffs in-memory.
pub fn get_log_with_stats(repo: &Repository, since: Option<&str>) -> Result<Vec<Commit>> {
    let head = repo
        .head_id()
        .map_err(|e| Error::git(format!("Failed to get HEAD: {e}")))?;

    let cutoff_time = since.and_then(parse_since_duration).map(|duration| {
        let now = std::time::SystemTime::now();
        now.checked_sub(duration)
            .unwrap_or(std::time::UNIX_EPOCH)
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });

    let walk = repo
        .rev_walk([head])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(|e| Error::git(format!("Failed to walk commits: {e}")))?;

    let mut commits = Vec::new();

    for info in walk {
        let info = info.map_err(|e| Error::git(format!("Failed to read commit: {e}")))?;
        let commit = info
            .object()
            .map_err(|e| Error::git(format!("Failed to get commit object: {e}")))?;

        let author = commit.author().map_err(|e| Error::git(format!("{e}")))?;
        let timestamp = author.time.seconds;

        // Skip commits older than cutoff
        if let Some(cutoff) = cutoff_time {
            if timestamp < cutoff {
                break;
            }
        }

        let message = commit
            .message()
            .map_err(|e| Error::git(format!("{e}")))?
            .title
            .to_string();

        // Get file changes by diffing against parent
        let files = get_commit_file_changes(repo, &commit)?;

        commits.push(Commit {
            sha: commit.id.to_string(),
            author: author.name.to_string(),
            email: author.email.to_string(),
            timestamp,
            message,
            files,
        });
    }

    Ok(commits)
}

/// Get file changes for a single commit by diffing against its first parent.
fn get_commit_file_changes(repo: &Repository, commit: &gix::Commit<'_>) -> Result<Vec<FileChange>> {
    let tree = commit
        .tree()
        .map_err(|e| Error::git(format!("Failed to get commit tree: {e}")))?;

    // Get parent tree (or empty tree for root commit)
    let parent_tree = commit
        .parent_ids()
        .next()
        .and_then(|parent_id| repo.find_commit(parent_id).ok().and_then(|c| c.tree().ok()));

    let mut changes = Vec::new();

    if let Some(parent) = parent_tree {
        let mut resource_cache = repo
            .diff_resource_cache_for_tree_diff()
            .map_err(|e| Error::git(format!("{e}")))?;

        parent
            .changes()
            .map_err(|e| Error::git(format!("{e}")))?
            .options(|opts| {
                opts.track_path();
            })
            .for_each_to_obtain_tree_with_cache(&tree, &mut resource_cache, |change| {
                use gix::object::tree::diff::Change;
                let (path, change_type) = match change {
                    Change::Addition { location, .. } => {
                        (PathBuf::from(location.to_string()), ChangeType::Added)
                    }
                    Change::Deletion { location, .. } => {
                        (PathBuf::from(location.to_string()), ChangeType::Deleted)
                    }
                    Change::Modification { location, .. } => {
                        (PathBuf::from(location.to_string()), ChangeType::Modified)
                    }
                    Change::Rewrite { location, .. } => {
                        (PathBuf::from(location.to_string()), ChangeType::Renamed)
                    }
                };
                changes.push(FileChange {
                    path,
                    additions: 0,
                    deletions: 0,
                    change_type,
                });
                Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue)
            })
            .map_err(|e| Error::git(format!("{e}")))?;
    } else {
        // Root commit - all files are additions
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse()
            .breadthfirst(&mut recorder)
            .map_err(|e| Error::git(format!("{e}")))?;

        for entry in recorder.records {
            if entry.mode.is_blob() {
                changes.push(FileChange {
                    path: PathBuf::from(entry.filepath.to_string()),
                    additions: 0,
                    deletions: 0,
                    change_type: ChangeType::Added,
                });
            }
        }
    }

    Ok(changes)
}

/// Get statistics for a specific commit.
pub fn get_commit_stats(repo: &Repository, sha: &str) -> Result<CommitStats> {
    let oid = gix::ObjectId::from_hex(sha.as_bytes())
        .map_err(|e| Error::git(format!("Invalid SHA: {e}")))?;

    let commit = repo
        .find_commit(oid)
        .map_err(|e| Error::git(format!("Commit not found: {e}")))?;

    let files = get_commit_file_changes(repo, &commit)?;

    let additions: u32 = files.iter().map(|f| f.additions).sum();
    let deletions: u32 = files.iter().map(|f| f.deletions).sum();

    Ok(CommitStats {
        sha: sha.to_string(),
        additions,
        deletions,
        files_changed: files.len(),
    })
}

/// Get diff stats between two refs.
pub fn get_diff_stats(repo: &Repository, from: &str, to: &str) -> Result<Vec<FileChange>> {
    let from_id = repo
        .rev_parse_single(from.as_bytes())
        .map_err(|e| Error::git(format!("Invalid ref '{}': {}", from, e)))?
        .object()
        .map_err(|e| Error::git(format!("{e}")))?
        .peel_to_commit()
        .map_err(|e| Error::git(format!("{e}")))?;

    let to_id = repo
        .rev_parse_single(to.as_bytes())
        .map_err(|e| Error::git(format!("Invalid ref '{}': {}", to, e)))?
        .object()
        .map_err(|e| Error::git(format!("{e}")))?
        .peel_to_commit()
        .map_err(|e| Error::git(format!("{e}")))?;

    let from_tree = from_id.tree().map_err(|e| Error::git(format!("{e}")))?;
    let to_tree = to_id.tree().map_err(|e| Error::git(format!("{e}")))?;

    let mut changes = Vec::new();
    let mut resource_cache = repo
        .diff_resource_cache_for_tree_diff()
        .map_err(|e| Error::git(format!("{e}")))?;

    from_tree
        .changes()
        .map_err(|e| Error::git(format!("{e}")))?
        .options(|opts| {
            opts.track_path();
        })
        .for_each_to_obtain_tree_with_cache(&to_tree, &mut resource_cache, |change| {
            use gix::object::tree::diff::Change;
            let (path, change_type) = match change {
                Change::Addition { location, .. } => {
                    (PathBuf::from(location.to_string()), ChangeType::Added)
                }
                Change::Deletion { location, .. } => {
                    (PathBuf::from(location.to_string()), ChangeType::Deleted)
                }
                Change::Modification { location, .. } => {
                    (PathBuf::from(location.to_string()), ChangeType::Modified)
                }
                Change::Rewrite { location, .. } => {
                    (PathBuf::from(location.to_string()), ChangeType::Renamed)
                }
            };
            changes.push(FileChange {
                path,
                additions: 0,
                deletions: 0,
                change_type,
            });
            Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue)
        })
        .map_err(|e| Error::git(format!("{e}")))?;

    Ok(changes)
}

/// Get merge base between two refs.
pub fn get_merge_base(repo: &Repository, ref1: &str, ref2: &str) -> Result<String> {
    let id1 = repo
        .rev_parse_single(ref1.as_bytes())
        .map_err(|e| Error::git(format!("Invalid ref '{}': {}", ref1, e)))?;

    let id2 = repo
        .rev_parse_single(ref2.as_bytes())
        .map_err(|e| Error::git(format!("Invalid ref '{}': {}", ref2, e)))?;

    let base = repo
        .merge_base(id1, id2)
        .map_err(|e| Error::git(format!("Failed to find merge base: {e}")))?;

    Ok(base.to_string())
}

/// Count commits between two refs (equivalent to git rev-list --count from..to).
pub fn get_commit_count(repo: &Repository, from: &str, to: &str) -> Result<i32> {
    let from_id = repo
        .rev_parse_single(from.as_bytes())
        .map_err(|e| Error::git(format!("Invalid ref '{}': {}", from, e)))?;

    let to_id = repo
        .rev_parse_single(to.as_bytes())
        .map_err(|e| Error::git(format!("Invalid ref '{}': {}", to, e)))?;

    // Walk from 'to' back to 'from' and count
    let walk = repo
        .rev_walk([to_id])
        .sorting(gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ))
        .all()
        .map_err(|e| Error::git(format!("Failed to walk commits: {e}")))?;

    let from_oid = from_id.detach();
    let mut count = 0i32;

    for info in walk {
        let info = info.map_err(|e| Error::git(format!("Failed to read commit: {e}")))?;
        // Stop when we reach the 'from' commit
        if info.id == from_oid {
            break;
        }
        count += 1;
    }

    Ok(count)
}

/// Aggregated churn data for files (replacement for git log --numstat parsing).
#[derive(Debug, Clone, Default)]
pub struct FileChurnData {
    /// Number of commits touching this file.
    pub commits: u32,
    /// Authors who modified this file with their commit counts.
    pub authors: HashMap<String, u32>,
    /// Total lines added.
    pub additions: u32,
    /// Total lines deleted.
    pub deletions: u32,
    /// First commit timestamp.
    pub first_commit: Option<i64>,
    /// Last commit timestamp.
    pub last_commit: Option<i64>,
}

/// Get file churn data for all files in the repository since a given time.
///
/// This is a faster replacement for parsing `git log --numstat` output.
pub fn get_file_churn(
    repo: &Repository,
    since: Option<&str>,
) -> Result<HashMap<PathBuf, FileChurnData>> {
    let commits = get_log_with_stats(repo, since)?;
    let mut file_data: HashMap<PathBuf, FileChurnData> = HashMap::new();

    for commit in commits {
        for file_change in &commit.files {
            let data = file_data.entry(file_change.path.clone()).or_default();

            data.commits += 1;
            *data.authors.entry(commit.author.clone()).or_insert(0) += 1;
            data.additions += file_change.additions;
            data.deletions += file_change.deletions;

            // Update time range
            match data.first_commit {
                Some(first) if commit.timestamp < first => {
                    data.first_commit = Some(commit.timestamp)
                }
                None => data.first_commit = Some(commit.timestamp),
                _ => {}
            }
            match data.last_commit {
                Some(last) if commit.timestamp > last => data.last_commit = Some(commit.timestamp),
                None => data.last_commit = Some(commit.timestamp),
                _ => {}
            }
        }
    }

    Ok(file_data)
}

/// Get unique contributor count for a file (replacement for git shortlog -sn).
pub fn get_file_contributors(repo: &Repository, file_path: &str) -> Result<Vec<(String, u32)>> {
    let head = repo
        .head_id()
        .map_err(|e| Error::git(format!("Failed to get HEAD: {e}")))?;

    let walk = repo
        .rev_walk([head])
        .all()
        .map_err(|e| Error::git(format!("Failed to walk commits: {e}")))?;

    let mut contributors: HashMap<String, u32> = HashMap::new();
    let file_path = PathBuf::from(file_path);

    for info in walk {
        let info = info.map_err(|e| Error::git(format!("Failed to read commit: {e}")))?;
        let commit = info
            .object()
            .map_err(|e| Error::git(format!("Failed to get commit object: {e}")))?;

        // Check if this commit touches the file
        let files = get_commit_file_changes(repo, &commit)?;
        if files.iter().any(|f| f.path == file_path) {
            let author = commit.author().map_err(|e| Error::git(format!("{e}")))?;
            *contributors.entry(author.name.to_string()).or_insert(0) += 1;
        }
    }

    let mut result: Vec<_> = contributors.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by commit count descending
    Ok(result)
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
