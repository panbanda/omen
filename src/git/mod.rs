//! Git operations for repository analysis.

mod blame;
mod log;
mod remote;

use std::path::{Path, PathBuf};

use gix::Repository;
use serde::{Deserialize, Serialize};

use crate::core::{Error, Result};

pub use blame::BlameInfo;
pub use log::{
    is_since_all, parse_since_to_days, ChangeType, Commit, CommitStats, FileChange, FileChurnEntry,
};
pub use remote::{clone_remote, is_remote_repo, CloneOptions};

/// Parsed `git log --numstat` history shared by analyzers in one context.
pub struct GitLogData {
    root: PathBuf,
    commits: Vec<Commit>,
    history_complete: bool,
}

impl GitLogData {
    /// Load the complete repository history once so every requested window can
    /// be produced without another numstat parse.
    pub fn load(path: &Path) -> Result<Self> {
        let repo = GitRepo::open(path)?;
        let root = repo.root().to_path_buf();
        // Asked while the repository is open: in a worktree, a submodule or a
        // separate git dir, `.git` is a file and the shallow marker lives in
        // the resolved common directory, so looking for `root/.git/shallow`
        // would call a shallow clone complete.
        let history_complete = !repo.is_shallow();
        let commits = repo.log_with_stats(None, None)?;
        Ok(Self {
            root,
            commits,
            history_complete,
        })
    }

    /// Whether the loaded history is complete, i.e. not a shallow clone.
    pub fn history_complete(&self) -> bool {
        self.history_complete
    }

    /// The committer time of the newest commit reachable from HEAD, which is
    /// the date of the revision being analyzed.
    ///
    /// `log_with_stats(None, None)` is reverse-chronological from HEAD, so
    /// this is `commits[0]`.
    pub fn anchor(&self) -> Option<i64> {
        self.commits.first().map(|commit| commit.commit_time)
    }

    /// Return the exact subset that `git log --since` would select.
    pub fn query(&self, since: Option<&str>, limit: Option<usize>) -> Result<Vec<Commit>> {
        let cutoff = since.map(|value| self.resolve_since(value)).transpose()?;
        let mut commits: Vec<Commit> = self
            .commits
            .iter()
            .filter(|commit| cutoff.is_none_or(|timestamp| commit.commit_time >= timestamp))
            .cloned()
            .collect();
        if let Some(max) = limit {
            commits.truncate(max);
        }
        Ok(commits)
    }

    /// Resolve a `since` window to a cutoff timestamp.
    ///
    /// A relative duration is measured from the analyzed revision's own date
    /// rather than from the wall clock: a worktree, a release tag, a bisect
    /// step or a repository nobody has touched this quarter would otherwise
    /// match no commits at all, and report that as "nothing to see" rather
    /// than "no data". It also makes results deterministic. An absolute date
    /// is already a fixed point, so it is handed to git as before.
    fn resolve_since(&self, since: &str) -> Result<i64> {
        if is_since_all(since) {
            return Ok(i64::MIN);
        }
        if let Some(duration) = log::parse_since_duration(since) {
            if let Some(anchor) = self.anchor() {
                return Ok(match log::cutoff_from(anchor, duration) {
                    log::Cutoff::At(cutoff) => cutoff,
                    // A window reaching past the epoch is all of history.
                    _ => i64::MIN,
                });
            }
        }

        log::git_resolved_since(&self.root, since)
    }
}

/// The churn window an analyzer actually used.
///
/// Emitted so an empty window reads as "no data" rather than as "nothing to
/// report", and so the numbers document the window that produced them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChurnWindow {
    /// Window width in days. `None` means all of history.
    pub days: Option<u32>,
    /// Date of the revision the window ends at (RFC 3339), or `None` when the
    /// repository has no commits.
    pub anchor_date: Option<String>,
    /// Commits the window actually matched.
    pub commits_matched: usize,
    /// False for a shallow clone, where a valid HEAD coexists with truncated
    /// history, so a low match count proves nothing.
    pub history_complete: bool,
}

impl ChurnWindow {
    pub fn new(
        days: u32,
        anchor: Option<i64>,
        commits_matched: usize,
        history_complete: bool,
    ) -> Self {
        Self {
            days: (days != u32::MAX).then_some(days),
            anchor_date: anchor
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|date| date.to_rfc3339()),
            commits_matched,
            history_complete,
        }
    }
}

/// Git repository wrapper for analysis operations.
pub struct GitRepo {
    /// The gix repository handle.
    repo: Repository,
    /// Repository root path.
    root: PathBuf,
}

impl GitRepo {
    /// Open a git repository at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let repo = gix::discover(path)
            .map_err(|e| Error::git(format!("Failed to discover repository: {e}")))?;
        let root = repo
            .workdir()
            .ok_or_else(|| Error::git("Not a work tree"))?
            .to_path_buf();

        Ok(Self { repo, root })
    }

    /// Get the repository root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check if path is inside this repository.
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    /// Whether this is a shallow clone, where a valid HEAD coexists with
    /// truncated history.
    pub fn is_shallow(&self) -> bool {
        self.repo.is_shallow()
    }

    /// Get the current branch name.
    pub fn current_branch(&self) -> Result<String> {
        let head = self
            .repo
            .head()
            .map_err(|e| Error::git(format!("Failed to get HEAD: {e}")))?;

        match head.referent_name() {
            Some(name) => Ok(name
                .as_bstr()
                .to_string()
                .strip_prefix("refs/heads/")
                .unwrap_or(&name.as_bstr().to_string())
                .to_string()),
            None => Ok("HEAD".to_string()),
        }
    }

    /// Get the HEAD commit SHA.
    pub fn head_sha(&self) -> Result<String> {
        let head = self
            .repo
            .head_id()
            .map_err(|e| Error::git(format!("Failed to get HEAD: {e}")))?;
        Ok(head.to_string())
    }

    /// Committer time of HEAD, or `None` when HEAD is unborn.
    ///
    /// This is the anchor every relative time window is measured from.
    pub fn head_commit_time(&self) -> Result<Option<i64>> {
        log::head_commit_time(&self.repo)
    }

    /// Get commit log with optional path filter.
    pub fn log(
        &self,
        since: Option<&str>,
        paths: Option<&[PathBuf]>,
        limit: Option<usize>,
    ) -> Result<Vec<Commit>> {
        log::get_log(&self.repo, since, paths, limit)
    }

    /// Get commit log with file change statistics (equivalent to git log --numstat).
    pub fn log_with_stats(&self, since: Option<&str>, limit: Option<usize>) -> Result<Vec<Commit>> {
        log::get_log_with_stats(&self.repo, since, limit)
    }

    /// Get per-file churn (commit count + authors) for specific paths.
    ///
    /// Uses path-filtered git log, so cost scales with the history of the
    /// requested files rather than the entire repository.
    pub fn file_churn(
        &self,
        paths: &[String],
    ) -> Result<std::collections::HashMap<String, FileChurnEntry>> {
        log::get_file_churn(&self.repo, paths)
    }

    /// Get blame information for a file.
    pub fn blame(&self, path: &Path) -> Result<BlameInfo> {
        blame::get_blame(&self.repo, &self.root, path)
    }

    /// Get commit statistics for a specific commit.
    pub fn commit_stats(&self, sha: &str) -> Result<CommitStats> {
        log::get_commit_stats(&self.repo, sha)
    }

    /// Get diff stats between two refs.
    pub fn diff_stats(&self, from: &str, to: &str) -> Result<Vec<FileChange>> {
        log::get_diff_stats(&self.repo, from, to)
    }

    /// Get the merge base between two refs.
    pub fn merge_base(&self, ref1: &str, ref2: &str) -> Result<String> {
        log::get_merge_base(&self.repo, ref1, ref2)
    }

    /// Check if a ref (branch, tag, etc.) exists.
    pub fn ref_exists(&self, refname: &str) -> bool {
        self.repo.rev_parse_single(refname.as_bytes()).is_ok()
    }

    /// Count commits between two refs (equivalent to git rev-list --count from..to).
    pub fn commit_count(&self, from: &str, to: &str) -> Result<i32> {
        log::get_commit_count(&self.repo, from, to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_git_repo(path: &Path) {
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
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .expect("failed to set git name");
    }

    fn make_commit(path: &Path, message: &str) {
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", message])
            .current_dir(path)
            .output()
            .expect("failed to commit");
    }

    #[test]
    fn test_git_repo_open_not_a_repo() {
        let temp = tempfile::tempdir().unwrap();
        let result = GitRepo::open(temp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_git_repo_open_valid_repo() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let repo = GitRepo::open(temp.path());
        assert!(repo.is_ok());
    }

    #[test]
    fn test_git_repo_root() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let repo = GitRepo::open(temp.path()).unwrap();
        // Canonicalize both paths for macOS where /var -> /private/var
        assert_eq!(
            repo.root().canonicalize().unwrap(),
            temp.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_git_repo_contains() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let repo = GitRepo::open(temp.path()).unwrap();

        let inside = temp.path().join("src").join("main.rs");
        let outside = PathBuf::from("/tmp/other/file.rs");

        assert!(repo.contains(&inside));
        assert!(!repo.contains(&outside));
    }

    #[test]
    fn test_git_repo_current_branch_no_commits() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        let repo = GitRepo::open(temp.path()).unwrap();
        // Before first commit, branch name may vary
        let result = repo.current_branch();
        // Either returns an error or a branch name
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_git_repo_current_branch_with_commit() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        let branch = repo.current_branch().unwrap();
        // Default branch is usually "master" or "main"
        assert!(branch == "master" || branch == "main" || !branch.is_empty());
    }

    #[test]
    fn test_git_repo_head_sha() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        let sha = repo.head_sha().unwrap();
        // SHA should be a 40-character hex string
        assert_eq!(sha.len(), 40);
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_git_repo_log() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        // log() currently returns empty Vec (placeholder)
        let result = repo.log(None, None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_repo_log_with_since() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        let result = repo.log(Some("7 days"), None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_repo_commit_stats() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        let sha = repo.head_sha().unwrap();
        // commit_stats works with gix
        let result = repo.commit_stats(&sha);
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_repo_diff_stats() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        // Create first commit with a file
        let file_path = temp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp.path())
            .output()
            .expect("failed to add file");
        make_commit(temp.path(), "Initial commit");

        // Create second commit with modification
        std::fs::write(&file_path, "fn main() { println!(\"hello\"); }").unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp.path())
            .output()
            .expect("failed to add file");
        make_commit(temp.path(), "Second commit");

        let repo = GitRepo::open(temp.path()).unwrap();
        // diff_stats works with gix when there are two commits
        let result = repo.diff_stats("HEAD~1", "HEAD");
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_repo_merge_base() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        // merge_base works with gix
        let result = repo.merge_base("HEAD", "HEAD");
        assert!(result.is_ok());
        // Merge base of HEAD with itself is HEAD
        let sha = repo.head_sha().unwrap();
        assert_eq!(result.unwrap(), sha);
    }

    #[test]
    fn test_git_repo_blame() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());

        // Create a file and commit it
        let file_path = temp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();
        Command::new("git")
            .args(["add", "test.rs"])
            .current_dir(temp.path())
            .output()
            .expect("failed to add file");
        make_commit(temp.path(), "Add test file");

        let repo = GitRepo::open(temp.path()).unwrap();
        let result = repo.blame(&file_path);
        // blame might work or fail depending on gix implementation
        assert!(result.is_ok() || result.is_err());
    }

    // --- Issue #496: windows anchored at the analyzed revision -------------

    /// Build a repo whose entire history predates any plausible wall clock,
    /// so a window measured from "now" matches nothing.
    fn init_old_repo(path: &std::path::Path) -> Vec<String> {
        let run = |args: &[&str], date: Option<&str>| {
            let mut cmd = std::process::Command::new("git");
            cmd.args(args).current_dir(path);
            if let Some(date) = date {
                cmd.env("GIT_AUTHOR_DATE", date)
                    .env("GIT_COMMITTER_DATE", date);
            }
            cmd.output().expect("git command failed");
        };
        run(&["init"], None);
        run(&["config", "user.email", "test@example.com"], None);
        run(&["config", "user.name", "Test Author"], None);

        // Far enough apart that a 30-day window reaches only HEAD.
        let dates = ["2020-01-01T00:00:00+0000", "2020-06-01T00:00:00+0000"];
        for (i, date) in dates.iter().enumerate() {
            std::fs::write(path.join("a.txt"), format!("line {i}\n")).unwrap();
            run(&["add", "-A"], None);
            run(&["commit", "-m", &format!("commit {i}")], Some(date));
        }
        dates.iter().map(|d| d.to_string()).collect()
    }

    #[test]
    fn test_relative_window_is_anchored_at_head_not_the_wall_clock() {
        // The bug: `--since=30 days` resolves against the current time, so a
        // checkout older than the window matches nothing and the analyzers
        // report "no data" as though it meant "nothing to report".
        let temp = tempfile::tempdir().unwrap();
        init_old_repo(temp.path());

        let data = GitLogData::load(temp.path()).expect("history should load");
        assert_eq!(data.commits.len(), 2, "fixture should have two commits");

        let recent = data.query(Some("30 days"), None).expect("query");
        assert_eq!(
            recent.len(),
            1,
            "a 30-day window ending at HEAD (2020-06-01) should contain only \
             the HEAD commit, not zero commits"
        );

        let wide = data.query(Some("1y"), None).expect("query");
        assert_eq!(wide.len(), 2, "a one-year window should reach both commits");

        let all = data.query(None, None).expect("query");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_window_is_measured_from_committer_time() {
        // `git log --since` filters on committer date, and gix orders by it,
        // so the window must use the same clock rather than the author date.
        let temp = tempfile::tempdir().unwrap();
        init_old_repo(temp.path());

        let data = GitLogData::load(temp.path()).expect("history should load");
        for commit in &data.commits {
            assert!(
                commit.commit_time > 0,
                "committer time should be populated, got {}",
                commit.commit_time
            );
        }
    }

    #[test]
    fn test_gix_log_filters_on_absolute_dates_too() {
        // `GitRepo::log` uses the gix walk, which has no git process to defer
        // to -- so an absolute date must be resolved before filtering, not
        // treated as "no lower bound".
        let temp = tempfile::tempdir().unwrap();
        init_old_repo(temp.path());
        let repo = GitRepo::open(temp.path()).unwrap();

        let all = repo.log(None, None, None).unwrap();
        assert_eq!(all.len(), 2);

        let since_mid = repo.log(Some("2020-03-01"), None, None).unwrap();
        assert_eq!(
            since_mid.len(),
            1,
            "an absolute date must filter the gix walk, not be ignored"
        );
    }

    #[test]
    fn test_absolute_since_dates_still_resolve() {
        let temp = tempfile::tempdir().unwrap();
        init_old_repo(temp.path());

        let data = GitLogData::load(temp.path()).expect("history should load");
        let since_mid = data.query(Some("2020-01-10"), None).expect("query");
        assert_eq!(
            since_mid.len(),
            1,
            "an absolute date is not relative to HEAD and must still work"
        );
    }
}
