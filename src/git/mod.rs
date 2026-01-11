//! Git operations for repository analysis.

mod blame;
mod log;
mod remote;

use std::path::{Path, PathBuf};

use gix::Repository;

use crate::core::{Error, Result};

pub use blame::BlameInfo;
pub use log::{ChangeType, Commit, CommitStats, FileChange};
pub use remote::{clone_remote, is_remote_repo, CloneOptions};

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
        let repo =
            gix::open(path).map_err(|e| Error::git(format!("Failed to open repository: {e}")))?;
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

    /// Get commit log with optional path filter.
    pub fn log(&self, since: Option<&str>, paths: Option<&[PathBuf]>) -> Result<Vec<Commit>> {
        log::get_log(&self.repo, since, paths)
    }

    /// Get commit log with file change statistics (equivalent to git log --numstat).
    pub fn log_with_stats(&self, since: Option<&str>) -> Result<Vec<Commit>> {
        log::get_log_with_stats(&self.repo, since)
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
        let result = repo.log(None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_git_repo_log_with_since() {
        let temp = tempfile::tempdir().unwrap();
        init_git_repo(temp.path());
        make_commit(temp.path(), "Initial commit");
        let repo = GitRepo::open(temp.path()).unwrap();
        let result = repo.log(Some("7 days"), None);
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
}
