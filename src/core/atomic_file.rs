//! Atomic file writes that do not follow a link at a guessed name.

use std::io::Write;
use std::path::Path;

/// The permissions of a file that `write_atomically` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileMode {
    /// Use these Unix permission bits. Other systems ignore them.
    Exactly(u32),
    /// Keep the permissions of the file that the write replaces. A new file
    /// gets these Unix permission bits.
    KeepExisting { new_file: u32 },
}

/// Write the file in one step: the old file stays until the new content is
/// complete. The temporary file has a random name and is new, so a link at a
/// guessed name is not followed.
pub fn write_atomically(path: &Path, content: &[u8], mode: FileMode) -> std::io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    pending.write_all(content)?;
    pending.as_file().sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bits = match mode {
            FileMode::Exactly(bits) => bits,
            FileMode::KeepExisting { new_file } => match std::fs::metadata(path) {
                Ok(existing) => existing.permissions().mode() & 0o7777,
                Err(_) => new_file,
            },
        };
        pending
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(bits))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    pending.persist(path).map_err(|error| error.error)?;
    Ok(())
}
