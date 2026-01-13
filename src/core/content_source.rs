//! Content source abstraction for reading file contents.
//!
//! This module provides a trait for abstracting how file contents are read,
//! allowing analysis to work with both filesystem files and git tree objects.

use std::path::{Path, PathBuf};

use super::Result;

/// Trait for reading file contents from different sources.
///
/// Implementations can read from:
/// - Filesystem (current working directory)
/// - Git tree objects (historical commits without checkout)
pub trait ContentSource: Send + Sync {
    /// Read the contents of a file at the given path.
    fn read(&self, path: &Path) -> Result<Vec<u8>>;
}

/// Reads file contents from the filesystem.
pub struct FilesystemSource {
    root: PathBuf,
}

impl FilesystemSource {
    /// Create a new filesystem source rooted at the given path.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl ContentSource for FilesystemSource {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let full_path = self.root.join(path);
        std::fs::read(&full_path).map_err(super::Error::Io)
    }
}

/// Reads file contents from a git tree object at a specific commit.
/// Does not require filesystem checkout - reads directly from git's object store.
///
/// This struct stores the repo path and tree ID, reopening the repo on each read.
/// This is thread-safe since each read is independent.
pub struct TreeSource {
    repo_path: PathBuf,
    tree_id: [u8; 20], // Store raw SHA bytes to avoid gix types in struct
}

impl TreeSource {
    /// Create a new tree source for the given repository at the specified commit.
    pub fn new(repo_path: impl AsRef<Path>, commit_sha: &str) -> Result<Self> {
        let repo_path = repo_path.as_ref().to_path_buf();
        let repo = gix::open(&repo_path)
            .map_err(|e| super::Error::git(format!("Failed to open repository: {e}")))?;

        let commit_id = repo
            .rev_parse_single(commit_sha.as_bytes())
            .map_err(|e| super::Error::git(format!("Failed to parse commit {commit_sha}: {e}")))?
            .detach();

        let commit = repo
            .find_object(commit_id)
            .map_err(|e| super::Error::git(format!("Failed to find commit: {e}")))?
            .try_into_commit()
            .map_err(|e| super::Error::git(format!("Not a commit: {e}")))?;

        let tree_oid = commit
            .tree_id()
            .map_err(|e| super::Error::git(format!("Failed to get tree id: {e}")))?;

        let mut tree_id = [0u8; 20];
        tree_id.copy_from_slice(tree_oid.as_bytes());

        Ok(Self { repo_path, tree_id })
    }

    /// Get the repository path.
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// Get the tree ID bytes.
    pub fn tree_id(&self) -> [u8; 20] {
        self.tree_id
    }

    /// List all files in the tree (recursively).
    pub fn list_files(&self) -> Result<Vec<PathBuf>> {
        let repo = gix::open(&self.repo_path)
            .map_err(|e| super::Error::git(format!("Failed to open repository: {e}")))?;

        let tree_oid = gix::ObjectId::from_bytes_or_panic(&self.tree_id);
        let tree = repo
            .find_object(tree_oid)
            .map_err(|e| super::Error::git(format!("Failed to find tree: {e}")))?
            .try_into_tree()
            .map_err(|e| super::Error::git(format!("Not a tree: {e}")))?;

        let mut files = Vec::new();
        self.collect_files(&repo, &tree, PathBuf::new(), &mut files)?;
        Ok(files)
    }

    fn collect_files(
        &self,
        repo: &gix::Repository,
        tree: &gix::Tree<'_>,
        prefix: PathBuf,
        files: &mut Vec<PathBuf>,
    ) -> Result<()> {
        for entry in tree.iter() {
            let entry =
                entry.map_err(|e| super::Error::git(format!("Failed to read entry: {e}")))?;
            let name = entry.filename().to_string();
            let path = prefix.join(&name);

            if entry.mode().is_tree() {
                // Recurse into subdirectory
                let subtree = repo
                    .find_object(entry.oid())
                    .map_err(|e| super::Error::git(format!("Failed to find subtree: {e}")))?
                    .try_into_tree()
                    .map_err(|e| super::Error::git(format!("Not a tree: {e}")))?;
                self.collect_files(repo, &subtree, path, files)?;
            } else if entry.mode().is_blob() {
                files.push(path);
            }
        }
        Ok(())
    }
}

impl ContentSource for TreeSource {
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let path_str = path.to_string_lossy();

        // Re-open repo for thread safety
        let repo = gix::open(&self.repo_path)
            .map_err(|e| super::Error::git(format!("Failed to open repository: {e}")))?;

        let tree_oid = gix::ObjectId::from_bytes_or_panic(&self.tree_id);

        // Get the tree
        let tree = repo
            .find_object(tree_oid)
            .map_err(|e| super::Error::git(format!("Failed to find tree: {e}")))?
            .try_into_tree()
            .map_err(|e| super::Error::git(format!("Not a tree: {e}")))?;

        // Find the entry in the tree
        let entry = tree
            .lookup_entry_by_path(path_str.as_ref())
            .map_err(|e| super::Error::git(format!("Failed to lookup {path_str}: {e}")))?
            .ok_or_else(|| super::Error::git(format!("File not found in tree: {path_str}")))?;

        // Get the blob content
        let object = entry
            .object()
            .map_err(|e| super::Error::git(format!("Failed to get object: {e}")))?;

        let blob = object
            .try_into_blob()
            .map_err(|_| super::Error::git(format!("Not a blob: {path_str}")))?;

        Ok(blob.data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_git_repo(path: &std::path::Path) {
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

    fn commit_file(
        path: &std::path::Path,
        filename: &str,
        content: &[u8],
        message: &str,
    ) -> String {
        std::fs::write(path.join(filename), content).unwrap();
        Command::new("git")
            .args(["add", filename])
            .current_dir(path)
            .output()
            .expect("failed to add file");
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(path)
            .output()
            .expect("failed to commit");
        // Get the commit SHA
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .expect("failed to get HEAD");
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn test_filesystem_source_reads_existing_file() {
        // Create a temp file
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        // FilesystemSource should read it
        let source = FilesystemSource::new(temp_dir.path());
        let content = source.read(Path::new("test.txt")).unwrap();

        assert_eq!(content, b"hello world");
    }

    #[test]
    fn test_filesystem_source_returns_error_for_missing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source = FilesystemSource::new(temp_dir.path());

        let result = source.read(Path::new("nonexistent.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_filesystem_source_reads_nested_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let nested_dir = temp_dir.path().join("src").join("lib");
        std::fs::create_dir_all(&nested_dir).unwrap();
        let file_path = nested_dir.join("main.rs");
        std::fs::write(&file_path, b"fn main() {}").unwrap();

        let source = FilesystemSource::new(temp_dir.path());
        let content = source.read(Path::new("src/lib/main.rs")).unwrap();

        assert_eq!(content, b"fn main() {}");
    }

    #[test]
    fn test_tree_source_reads_file_at_commit() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_git_repo(temp_dir.path());

        // Create first commit with a file
        let sha1 = commit_file(temp_dir.path(), "test.txt", b"version 1", "First commit");

        // Create second commit with modified file
        let _sha2 = commit_file(temp_dir.path(), "test.txt", b"version 2", "Second commit");

        // TreeSource should read the file at the FIRST commit (historical)
        let source = TreeSource::new(temp_dir.path(), &sha1).unwrap();
        let content = source.read(Path::new("test.txt")).unwrap();

        assert_eq!(content, b"version 1");
    }

    #[test]
    fn test_tree_source_reads_file_at_different_commit() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_git_repo(temp_dir.path());

        let sha1 = commit_file(temp_dir.path(), "test.txt", b"version 1", "First commit");
        let sha2 = commit_file(temp_dir.path(), "test.txt", b"version 2", "Second commit");

        // Read from first commit
        let source1 = TreeSource::new(temp_dir.path(), &sha1).unwrap();
        assert_eq!(source1.read(Path::new("test.txt")).unwrap(), b"version 1");

        // Read from second commit
        let source2 = TreeSource::new(temp_dir.path(), &sha2).unwrap();
        assert_eq!(source2.read(Path::new("test.txt")).unwrap(), b"version 2");
    }

    #[test]
    fn test_tree_source_returns_error_for_missing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_git_repo(temp_dir.path());

        let sha = commit_file(temp_dir.path(), "exists.txt", b"content", "Commit");

        let source = TreeSource::new(temp_dir.path(), &sha).unwrap();
        let result = source.read(Path::new("nonexistent.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_tree_source_reads_nested_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_git_repo(temp_dir.path());

        // Create nested directory and file
        let nested_dir = temp_dir.path().join("src").join("lib");
        std::fs::create_dir_all(&nested_dir).unwrap();
        std::fs::write(nested_dir.join("main.rs"), b"fn main() {}").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "Add nested file"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to commit");
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to get HEAD");
        let sha = String::from_utf8(output.stdout).unwrap().trim().to_string();

        let source = TreeSource::new(temp_dir.path(), &sha).unwrap();
        let content = source.read(Path::new("src/lib/main.rs")).unwrap();

        assert_eq!(content, b"fn main() {}");
    }

    #[test]
    fn test_tree_source_lists_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_git_repo(temp_dir.path());

        // Create several files
        std::fs::write(temp_dir.path().join("README.md"), b"# Hello").unwrap();
        std::fs::write(temp_dir.path().join("main.rs"), b"fn main() {}").unwrap();
        let src_dir = temp_dir.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("lib.rs"), b"pub mod foo;").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "Add files"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to commit");
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to get HEAD");
        let sha = String::from_utf8(output.stdout).unwrap().trim().to_string();

        let source = TreeSource::new(temp_dir.path(), &sha).unwrap();
        let files = source.list_files().unwrap();

        assert!(files.contains(&PathBuf::from("README.md")));
        assert!(files.contains(&PathBuf::from("main.rs")));
        assert!(files.contains(&PathBuf::from("src/lib.rs")));
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_tree_source_lists_files_at_historical_commit() {
        let temp_dir = tempfile::tempdir().unwrap();
        init_git_repo(temp_dir.path());

        // First commit: one file
        std::fs::write(temp_dir.path().join("first.txt"), b"first").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "First commit"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to commit");
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to get HEAD");
        let sha1 = String::from_utf8(output.stdout).unwrap().trim().to_string();

        // Second commit: add another file
        std::fs::write(temp_dir.path().join("second.txt"), b"second").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to add");
        Command::new("git")
            .args(["commit", "-m", "Second commit"])
            .current_dir(temp_dir.path())
            .output()
            .expect("failed to commit");

        // List files at first commit - should only have one file
        let source = TreeSource::new(temp_dir.path(), &sha1).unwrap();
        let files = source.list_files().unwrap();

        assert_eq!(files.len(), 1);
        assert!(files.contains(&PathBuf::from("first.txt")));
    }
}
