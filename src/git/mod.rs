//! Git operations for repository analysis.

mod blame;
mod log;
mod remote;

use std::path::{Path, PathBuf};

use gix::Repository;

use crate::core::{Error, Result};

pub use blame::BlameInfo;
pub use log::{Commit, CommitStats, FileChange};
pub use remote::clone_remote;

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
            .work_dir()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_repo_open_not_a_repo() {
        let temp = tempfile::tempdir().unwrap();
        let result = GitRepo::open(temp.path());
        assert!(result.is_err());
    }
}
