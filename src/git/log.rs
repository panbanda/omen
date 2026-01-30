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
///
/// This is a fast gix-based implementation that avoids spawning git CLI.
/// If `paths` is provided, only commits that touch those paths are returned.
pub fn get_log(
    repo: &Repository,
    since: Option<&str>,
    paths: Option<&[PathBuf]>,
    limit: Option<usize>,
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
        let timestamp = author.seconds();

        // Skip commits older than cutoff
        if let Some(cutoff) = cutoff_time {
            if timestamp < cutoff {
                break;
            }
        }

        // If path filter is specified, check if commit touches any of the paths
        if let Some(filter_paths) = paths {
            let file_changes = get_commit_file_changes(repo, &commit)?;
            let matches_filter = file_changes.iter().any(|change| {
                filter_paths
                    .iter()
                    .any(|filter| change.path == *filter || change.path.starts_with(filter))
            });
            if !matches_filter {
                continue;
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

        if let Some(max) = limit {
            if commits.len() >= max {
                break;
            }
        }
    }

    if let Some(max) = limit {
        commits.truncate(max);
    }

    Ok(commits)
}

/// Returns true if the since string means "all history" (no time limit).
pub fn is_since_all(since: &str) -> bool {
    matches!(since.trim().to_lowercase().as_str(), "all" | "forever")
}

/// Parse "since" duration strings like "30 days ago", "1 week", "6m", "1y" to days.
///
/// Returns the number of days, or None if the format is invalid.
/// Returns None for "all" (meaning no time limit).
pub fn parse_since_to_days(since: &str) -> Option<u32> {
    if is_since_all(since) {
        return None;
    }
    parse_since_duration(since).map(|d| (d.as_secs() / 86400) as u32)
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
        "d" | "day" | "days" => num * 86400,
        "w" | "wk" | "wks" | "week" | "weeks" => num * 604800,
        "m" | "mo" | "mon" | "month" | "months" => num * 2592000, // 30 days
        "y" | "yr" | "yrs" | "year" | "years" => num * 31536000,  // 365 days
        _ => return None,
    };

    Some(std::time::Duration::from_secs(secs))
}

/// Get commit log with file change statistics (numstat equivalent).
///
/// Uses git CLI for performance - gix tree diff is ~160x slower.
pub fn get_log_with_stats(
    repo: &Repository,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<Commit>> {
    let repo_path = repo
        .workdir()
        .ok_or_else(|| Error::git("Not a work tree"))?;

    // Build git log command with numstat
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(repo_path);
    cmd.args(["log", "--format=%H|%an|%ae|%at|%s", "--numstat"]);

    if let Some(since_str) = since {
        cmd.arg(format!("--since={}", since_str));
    }

    if let Some(max) = limit {
        cmd.arg(format!("-n{}", max));
    }

    let output = cmd
        .output()
        .map_err(|e| Error::git(format!("Failed to run git log: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::git(format!("git log failed: {}", stderr)));
    }

    let mut commits = parse_git_log_numstat(&output.stdout)?;

    if let Some(max) = limit {
        commits.truncate(max);
    }

    Ok(commits)
}

/// Parse git log --numstat output into Commit structs.
fn parse_git_log_numstat(output: &[u8]) -> Result<Vec<Commit>> {
    use std::io::{BufRead, BufReader};

    let mut commits = Vec::new();
    let reader = BufReader::new(output);

    let mut current_commit: Option<Commit> = None;

    for line in reader.lines() {
        let line = line.map_err(|e| Error::git(format!("Failed to read git output: {}", e)))?;

        if line.is_empty() {
            continue;
        }

        // Check if this is a commit line (contains |)
        if line.contains('|') {
            let parts: Vec<&str> = line.splitn(5, '|').collect();
            if parts.len() == 5 {
                // Save previous commit
                if let Some(commit) = current_commit.take() {
                    commits.push(commit);
                }

                // Parse new commit
                let sha = parts[0].to_string();
                let author = parts[1].to_string();
                let email = parts[2].to_string();
                let timestamp: i64 = parts[3].parse().unwrap_or(0);
                let message = parts[4].to_string();

                current_commit = Some(Commit {
                    sha,
                    author,
                    email,
                    timestamp,
                    message,
                    files: Vec::new(),
                });
                continue;
            }
        }

        // This is a numstat line: added\tdeleted\tfilepath
        if let Some(ref mut commit) = current_commit {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() == 3 {
                let (added_str, deleted_str, path) = (parts[0], parts[1], parts[2]);

                // Skip binary files (shown as "-")
                if added_str == "-" || deleted_str == "-" {
                    continue;
                }

                let additions: u32 = added_str.parse().unwrap_or(0);
                let deletions: u32 = deleted_str.parse().unwrap_or(0);

                commit.files.push(FileChange {
                    path: PathBuf::from(path),
                    additions,
                    deletions,
                    change_type: if additions > 0 && deletions > 0 {
                        ChangeType::Modified
                    } else if additions > 0 {
                        ChangeType::Added
                    } else {
                        ChangeType::Deleted
                    },
                });
            }
        }
    }

    // Don't forget the last commit
    if let Some(commit) = current_commit {
        commits.push(commit);
    }

    Ok(commits)
}

/// Get commit log with file change statistics using gix (slower but pure Rust).
///
/// Note: This is ~160x slower than CLI for large repos. Use get_log_with_stats() for performance.
#[allow(dead_code)]
pub fn get_log_with_stats_gix(
    repo: &Repository,
    since: Option<&str>,
    limit: Option<usize>,
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
        let timestamp = author.seconds();

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

        if let Some(max) = limit {
            if commits.len() >= max {
                break;
            }
        }
    }

    if let Some(max) = limit {
        commits.truncate(max);
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
                let (path, change_type, is_blob) = match change {
                    Change::Addition {
                        location,
                        entry_mode,
                        ..
                    } => (
                        PathBuf::from(location.to_string()),
                        ChangeType::Added,
                        entry_mode.is_blob(),
                    ),
                    Change::Deletion {
                        location,
                        entry_mode,
                        ..
                    } => (
                        PathBuf::from(location.to_string()),
                        ChangeType::Deleted,
                        entry_mode.is_blob(),
                    ),
                    Change::Modification {
                        location,
                        entry_mode,
                        ..
                    } => (
                        PathBuf::from(location.to_string()),
                        ChangeType::Modified,
                        entry_mode.is_blob(),
                    ),
                    Change::Rewrite {
                        location,
                        entry_mode,
                        ..
                    } => (
                        PathBuf::from(location.to_string()),
                        ChangeType::Renamed,
                        entry_mode.is_blob(),
                    ),
                };
                // Only include blob (file) entries, not tree (directory) entries
                if is_blob {
                    changes.push(FileChange {
                        path,
                        additions: 0,
                        deletions: 0,
                        change_type,
                    });
                }
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
            let (path, change_type, is_blob) = match change {
                Change::Addition {
                    location,
                    entry_mode,
                    ..
                } => (
                    PathBuf::from(location.to_string()),
                    ChangeType::Added,
                    entry_mode.is_blob(),
                ),
                Change::Deletion {
                    location,
                    entry_mode,
                    ..
                } => (
                    PathBuf::from(location.to_string()),
                    ChangeType::Deleted,
                    entry_mode.is_blob(),
                ),
                Change::Modification {
                    location,
                    entry_mode,
                    ..
                } => (
                    PathBuf::from(location.to_string()),
                    ChangeType::Modified,
                    entry_mode.is_blob(),
                ),
                Change::Rewrite {
                    location,
                    entry_mode,
                    ..
                } => (
                    PathBuf::from(location.to_string()),
                    ChangeType::Renamed,
                    entry_mode.is_blob(),
                ),
            };
            // Only include blob (file) entries, not tree (directory) entries
            if is_blob {
                changes.push(FileChange {
                    path,
                    additions: 0,
                    deletions: 0,
                    change_type,
                });
            }
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

    #[test]
    fn test_get_log_filters_by_path() {
        use std::process::Command;

        // Create a temp directory for the test repo
        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .expect("failed to init git repo");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .expect("failed to set git email");
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .expect("failed to set git name");

        // Create file1.rs and commit it
        std::fs::write(repo_path.join("file1.rs"), "fn file1() {}").unwrap();
        Command::new("git")
            .args(["add", "file1.rs"])
            .current_dir(repo_path)
            .output()
            .expect("failed to add file1");
        Command::new("git")
            .args(["commit", "-m", "Add file1"])
            .current_dir(repo_path)
            .output()
            .expect("failed to commit file1");

        // Create file2.rs and commit it
        std::fs::write(repo_path.join("file2.rs"), "fn file2() {}").unwrap();
        Command::new("git")
            .args(["add", "file2.rs"])
            .current_dir(repo_path)
            .output()
            .expect("failed to add file2");
        Command::new("git")
            .args(["commit", "-m", "Add file2"])
            .current_dir(repo_path)
            .output()
            .expect("failed to commit file2");

        // Modify file1.rs and commit it
        std::fs::write(
            repo_path.join("file1.rs"),
            "fn file1() { println!(\"modified\"); }",
        )
        .unwrap();
        Command::new("git")
            .args(["add", "file1.rs"])
            .current_dir(repo_path)
            .output()
            .expect("failed to add modified file1");
        Command::new("git")
            .args(["commit", "-m", "Modify file1"])
            .current_dir(repo_path)
            .output()
            .expect("failed to commit modified file1");

        // Open the repo with gix
        let repo = gix::open(repo_path).expect("failed to open repo");

        // Get log for file1.rs only - should return 2 commits (Add file1, Modify file1)
        let file1_path = PathBuf::from("file1.rs");
        let paths = [file1_path];
        let commits = get_log(&repo, None, Some(&paths), None).expect("failed to get log");

        // After fix: Should return only 2 commits that touch file1.rs
        assert_eq!(
            commits.len(),
            2,
            "Expected 2 commits touching file1.rs, got {} commits: {:?}",
            commits.len(),
            commits.iter().map(|c| &c.message).collect::<Vec<_>>()
        );

        // Verify the commits are the right ones (trim whitespace from messages)
        let messages: Vec<_> = commits.iter().map(|c| c.message.trim()).collect();
        assert!(
            messages.iter().any(|m| m.contains("Modify file1")),
            "Should contain 'Modify file1' commit, got: {:?}",
            messages
        );
        assert!(
            messages.iter().any(|m| m.contains("Add file1")),
            "Should contain 'Add file1' commit, got: {:?}",
            messages
        );
        assert!(
            !messages.iter().any(|m| m.contains("Add file2")),
            "Should NOT contain 'Add file2' commit, got: {:?}",
            messages
        );
    }

    #[test]
    fn test_parse_since_to_days() {
        // Test years
        assert_eq!(parse_since_to_days("1y"), Some(365));
        assert_eq!(parse_since_to_days("10y"), Some(3650));
        assert_eq!(parse_since_to_days("1 year"), Some(365));
        assert_eq!(parse_since_to_days("2 years"), Some(730));

        // Test months
        assert_eq!(parse_since_to_days("6m"), Some(180));
        assert_eq!(parse_since_to_days("6mo"), Some(180));
        assert_eq!(parse_since_to_days("1 month"), Some(30));
        assert_eq!(parse_since_to_days("3 months"), Some(90));

        // Test weeks
        assert_eq!(parse_since_to_days("1w"), Some(7));
        assert_eq!(parse_since_to_days("2 weeks"), Some(14));

        // Test days
        assert_eq!(parse_since_to_days("30 days"), Some(30));
        assert_eq!(parse_since_to_days("90d"), Some(90));

        // Test "all" returns None (no time limit)
        assert_eq!(parse_since_to_days("all"), None);
        assert_eq!(parse_since_to_days("ALL"), None);
        assert_eq!(parse_since_to_days("forever"), None);

        // Test invalid
        assert_eq!(parse_since_to_days("invalid"), None);
        assert_eq!(parse_since_to_days(""), None);
    }

    #[test]
    fn test_is_since_all() {
        assert!(is_since_all("all"));
        assert!(is_since_all("ALL"));
        assert!(is_since_all("All"));
        assert!(is_since_all("forever"));
        assert!(is_since_all("  all  "));
        assert!(!is_since_all("1y"));
        assert!(!is_since_all(""));
        assert!(!is_since_all("6m"));
    }

    #[test]
    fn test_get_log_with_stats_passes_limit_flag() {
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path();

        // Initialize git repo with multiple commits
        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .expect("failed to init git repo");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .expect("failed to set git email");
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .expect("failed to set git name");

        for i in 0..5 {
            let filename = format!("file{}.rs", i);
            std::fs::write(repo_path.join(&filename), format!("fn f{}() {{}}", i)).unwrap();
            Command::new("git")
                .args(["add", &filename])
                .current_dir(repo_path)
                .output()
                .expect("failed to add file");
            Command::new("git")
                .args(["commit", "-m", &format!("Add file{}", i)])
                .current_dir(repo_path)
                .output()
                .expect("failed to commit");
        }

        let repo = gix::open(repo_path).expect("failed to open repo");

        // Without limit: all 5 commits
        let all = get_log_with_stats(&repo, None, None).expect("failed to get log");
        assert_eq!(all.len(), 5);

        // With limit of 2: only 2 commits
        let limited = get_log_with_stats(&repo, None, Some(2)).expect("failed to get limited log");
        assert_eq!(
            limited.len(),
            2,
            "Expected 2 commits with limit=2, got {}",
            limited.len()
        );
    }

    #[test]
    fn test_get_log_respects_limit() {
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path();

        Command::new("git")
            .args(["init"])
            .current_dir(repo_path)
            .output()
            .expect("failed to init git repo");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(repo_path)
            .output()
            .expect("failed to set git email");
        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(repo_path)
            .output()
            .expect("failed to set git name");

        for i in 0..5 {
            let filename = format!("file{}.rs", i);
            std::fs::write(repo_path.join(&filename), format!("fn f{}() {{}}", i)).unwrap();
            Command::new("git")
                .args(["add", &filename])
                .current_dir(repo_path)
                .output()
                .expect("failed to add file");
            Command::new("git")
                .args(["commit", "-m", &format!("Add file{}", i)])
                .current_dir(repo_path)
                .output()
                .expect("failed to commit");
        }

        let repo = gix::open(repo_path).expect("failed to open repo");

        // Without limit: all 5 commits
        let all = get_log(&repo, None, None, None).expect("failed to get log");
        assert_eq!(all.len(), 5);

        // With limit of 3: only 3 commits
        let limited = get_log(&repo, None, None, Some(3)).expect("failed to get limited log");
        assert_eq!(
            limited.len(),
            3,
            "Expected 3 commits with limit=3, got {}",
            limited.len()
        );
    }
}
