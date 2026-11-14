//! Atomic file writes that do not follow a link at a guessed name.

use std::io::Write;
use std::path::Path;

/// Write the file in one step: the old file stays until the new content is
/// complete. The temporary file has a random name and is new, so a link at a
/// guessed name is not followed.
///
/// The write replaces the file. Use it for output that omen owns, not for a
/// source file of the user. Other systems ignore `unix_mode`.
pub fn write_atomically(path: &Path, content: &[u8], unix_mode: u32) -> std::io::Result<()> {
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
        pending
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(unix_mode))?;
    }
    #[cfg(not(unix))]
    let _ = unix_mode;
    pending.persist(path).map_err(|error| error.error)?;
    Ok(())
}
