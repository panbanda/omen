//! File safety mechanisms for mutation testing.
//!
//! Provides RAII-based guards to ensure source files are always restored
//! to their original state, even if a panic occurs during mutation testing.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::{Error, Result};

/// RAII guard that ensures a file is restored to its original content.
///
/// When the guard is dropped (whether normally or due to panic), it restores
/// the original file content.
pub struct MutationGuard {
    /// Path to the file being mutated.
    path: PathBuf,
    /// Original file content (backup).
    original: Vec<u8>,
    /// Whether the file has been modified.
    modified: bool,
}

impl MutationGuard {
    /// Create a new mutation guard for the given file.
    ///
    /// Reads and stores the original file content for later restoration.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let original = fs::read(&path).map_err(Error::Io)?;

        Ok(Self {
            path,
            original,
            modified: false,
        })
    }

    /// Apply a mutation to the file.
    ///
    /// This writes the mutated content to the file atomically.
    pub fn apply(&mut self, content: &[u8]) -> Result<()> {
        atomic_write(&self.path, content)?;
        self.modified = true;
        Ok(())
    }

    /// Get the original file content.
    pub fn original(&self) -> &[u8] {
        &self.original
    }

    /// Get the file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if the file has been modified.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Restore the original content without dropping the guard.
    ///
    /// This is useful for running multiple mutations in sequence.
    pub fn restore(&mut self) -> Result<()> {
        if self.modified {
            atomic_write(&self.path, &self.original)?;
            self.modified = false;
        }
        Ok(())
    }
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        if self.modified {
            // Best-effort restoration - we can't propagate errors from drop
            let _ = fs::write(&self.path, &self.original);
        }
    }
}

/// Write content to a file atomically.
///
/// This writes to a temporary file first, then renames it to the target path.
/// This ensures that the file is never in a partially-written state.
pub fn atomic_write(path: impl AsRef<Path>, content: &[u8]) -> Result<()> {
    let path = path.as_ref();

    // Get the parent directory for the temp file
    let parent = path.parent().unwrap_or(Path::new("."));

    // Create a temporary file in the same directory
    let temp_path = parent.join(format!(".omen-mutation-{}.tmp", std::process::id()));

    // Write to temp file
    let mut file = File::create(&temp_path).map_err(Error::Io)?;
    file.write_all(content).map_err(Error::Io)?;
    file.sync_all().map_err(Error::Io)?;
    drop(file);

    // Rename temp file to target (atomic on POSIX, near-atomic on Windows)
    fs::rename(&temp_path, path).map_err(Error::Io)?;

    Ok(())
}

/// Check if there are uncommitted changes in the given file.
///
/// This is a safety check to warn users before mutating files with uncommitted changes.
pub fn has_uncommitted_changes(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();

    // Try to find git root
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d.join(".git").exists() {
            // Found git root, check if file has uncommitted changes
            let output = std::process::Command::new("git")
                .args(["status", "--porcelain"])
                .arg(path)
                .current_dir(d)
                .output();

            if let Ok(output) = output {
                return !output.stdout.is_empty();
            }
            return false;
        }
        dir = d.parent();
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_mutation_guard_new() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let guard = MutationGuard::new(&file_path).unwrap();

        assert_eq!(guard.original(), b"original content");
        assert_eq!(guard.path(), file_path);
        assert!(!guard.is_modified());
    }

    #[test]
    fn test_mutation_guard_new_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("nonexistent.rs");

        let result = MutationGuard::new(&file_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_mutation_guard_apply() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated content").unwrap();

        assert!(guard.is_modified());

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"mutated content");
    }

    #[test]
    fn test_mutation_guard_restore() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();
        guard.apply(b"mutated content").unwrap();
        guard.restore().unwrap();

        assert!(!guard.is_modified());

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");
    }

    #[test]
    fn test_mutation_guard_drop_restores() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        {
            let mut guard = MutationGuard::new(&file_path).unwrap();
            guard.apply(b"mutated content").unwrap();
            // Guard dropped here
        }

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");
    }

    #[test]
    fn test_mutation_guard_drop_does_nothing_if_not_modified() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        {
            let _guard = MutationGuard::new(&file_path).unwrap();
            // Guard dropped without modification
        }

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"original content");
    }

    #[test]
    fn test_mutation_guard_multiple_mutations() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"original content").unwrap();

        let mut guard = MutationGuard::new(&file_path).unwrap();

        // First mutation
        guard.apply(b"mutation 1").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"mutation 1");

        guard.restore().unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original content");

        // Second mutation
        guard.apply(b"mutation 2").unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"mutation 2");

        guard.restore().unwrap();
        assert_eq!(fs::read(&file_path).unwrap(), b"original content");
    }

    #[test]
    fn test_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        atomic_write(&file_path, b"test content").unwrap();

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"test content");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"old content").unwrap();

        atomic_write(&file_path, b"new content").unwrap();

        let content = fs::read(&file_path).unwrap();
        assert_eq!(content, b"new content");
    }

    #[test]
    fn test_atomic_write_no_temp_file_left() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");

        atomic_write(&file_path, b"content").unwrap();

        // Check no temp files left behind
        let entries: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "test.rs");
    }

    #[test]
    fn test_has_uncommitted_changes_no_git() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.rs");
        fs::write(&file_path, b"content").unwrap();

        // Should return false when not in a git repo
        assert!(!has_uncommitted_changes(&file_path));
    }
}
