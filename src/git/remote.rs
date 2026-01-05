//! Remote repository cloning.

use std::path::{Path, PathBuf};

use crate::core::{Error, Result};

/// Clone options for remote repositories.
#[derive(Debug, Clone, Default)]
pub struct CloneOptions {
    /// Use shallow clone (--depth 1).
    pub shallow: bool,
    /// Specific branch or tag to clone.
    pub reference: Option<String>,
    /// Target directory (defaults to temp dir).
    pub target: Option<PathBuf>,
}

/// Clone a remote repository.
///
/// Supports:
/// - GitHub shorthand: `owner/repo`
/// - Full URLs: `https://github.com/owner/repo`
/// - With ref: `owner/repo@v1.0.0`
pub fn clone_remote(url: &str, options: CloneOptions) -> Result<PathBuf> {
    let (repo_url, reference) = parse_remote_url(url)?;

    let target = options.target.unwrap_or_else(|| {
        let temp_dir = std::env::temp_dir().join("omen-repos");
        std::fs::create_dir_all(&temp_dir).ok();
        temp_dir.join(sanitize_repo_name(&repo_url))
    });

    // Use gix for cloning
    let mut prepare = gix::prepare_clone(repo_url.clone(), &target)
        .map_err(|e| Error::Remote(format!("Failed to prepare clone: {e}")))?;

    if options.shallow {
        prepare = prepare.with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
            std::num::NonZeroU32::new(1).unwrap(),
        ));
    }

    let (_repo, _outcome) = prepare
        .fetch_only(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| Error::Remote(format!("Failed to clone: {e}")))?;

    // Checkout the specific reference if provided
    let ref_to_checkout = reference.or(options.reference);
    if let Some(ref_name) = ref_to_checkout {
        checkout_ref(&target, &ref_name)?;
    }

    Ok(target)
}

/// Parse a remote URL, extracting any reference suffix.
fn parse_remote_url(url: &str) -> Result<(String, Option<String>)> {
    // Check for @ref suffix
    let (base, reference) = if let Some(at_pos) = url.rfind('@') {
        let base = &url[..at_pos];
        let reference = &url[at_pos + 1..];
        (base, Some(reference.to_string()))
    } else {
        (url, None)
    };

    // Convert shorthand to full URL
    let repo_url = if base.starts_with("http://") || base.starts_with("https://") {
        base.to_string()
    } else if base.starts_with("github.com/") {
        format!("https://{base}")
    } else if base.contains('/') && !base.contains("://") {
        // Assume GitHub shorthand: owner/repo
        format!("https://github.com/{base}")
    } else {
        return Err(Error::Remote(format!("Invalid repository URL: {url}")));
    };

    Ok((repo_url, reference))
}

/// Checkout a specific ref in a repository.
fn checkout_ref(repo_path: &Path, reference: &str) -> Result<()> {
    // Use git command for checkout since gix checkout is complex
    let output = std::process::Command::new("git")
        .args(["checkout", reference])
        .current_dir(repo_path)
        .output()
        .map_err(|e| Error::Remote(format!("Failed to run git checkout: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Remote(format!(
            "Failed to checkout {reference}: {stderr}"
        )));
    }

    Ok(())
}

/// Sanitize repository name for use as directory name.
fn sanitize_repo_name(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit('/')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("-")
        .replace(".git", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_shorthand() {
        let (url, reference) = parse_remote_url("owner/repo").unwrap();
        assert_eq!(url, "https://github.com/owner/repo");
        assert!(reference.is_none());
    }

    #[test]
    fn test_parse_github_shorthand_with_ref() {
        let (url, reference) = parse_remote_url("owner/repo@v1.0.0").unwrap();
        assert_eq!(url, "https://github.com/owner/repo");
        assert_eq!(reference, Some("v1.0.0".to_string()));
    }

    #[test]
    fn test_parse_full_url() {
        let (url, reference) = parse_remote_url("https://github.com/owner/repo").unwrap();
        assert_eq!(url, "https://github.com/owner/repo");
        assert!(reference.is_none());
    }

    #[test]
    fn test_sanitize_repo_name() {
        assert_eq!(
            sanitize_repo_name("https://github.com/owner/repo"),
            "owner-repo"
        );
        assert_eq!(
            sanitize_repo_name("https://github.com/owner/repo.git"),
            "owner-repo"
        );
    }
}
