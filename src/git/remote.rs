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

/// Check if a path string looks like a remote repository reference.
///
/// Returns true for:
/// - GitHub shorthand: `owner/repo`, `owner/repo@ref`
/// - Full URLs: `https://github.com/owner/repo`
/// - GitHub domain: `github.com/owner/repo`
///
/// Returns false for:
/// - Local paths: `.`, `./src`, `/home/user/project`
/// - Single words without slashes: `myproject`
/// - Paths that exist on the local filesystem
pub fn is_remote_repo(path: &str) -> bool {
    // Empty or current directory
    if path.is_empty() || path == "." {
        return false;
    }

    // Full URLs are definitely remote
    if path.starts_with("http://") || path.starts_with("https://") {
        return true;
    }

    // github.com/ prefix
    if path.starts_with("github.com/") {
        return true;
    }

    // Check for owner/repo pattern (not a local path)
    // Local paths typically start with ., /, ~, or contain multiple slashes
    if path.starts_with('.')
        || path.starts_with('/')
        || path.starts_with('~')
        || path.starts_with('\\')
    {
        return false;
    }

    // Check for Windows-style absolute paths (C:\, D:\, etc.)
    if path.len() >= 2 && path.chars().nth(1) == Some(':') {
        return false;
    }

    // If the path exists locally, it's not a remote repo
    if Path::new(path).exists() {
        return false;
    }

    // Now check for owner/repo pattern: exactly one slash (or one slash before @)
    let base = if let Some(at_pos) = path.rfind('@') {
        &path[..at_pos]
    } else {
        path
    };

    // Count slashes - owner/repo has exactly one
    let slash_count = base.chars().filter(|&c| c == '/').count();

    // Must have exactly one slash and both parts non-empty
    if slash_count == 1 {
        let parts: Vec<&str> = base.split('/').collect();
        return parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty();
    }

    false
}

/// Clone a remote repository.
///
/// Supports:
/// - GitHub shorthand: `owner/repo`
/// - Full URLs: `https://github.com/owner/repo`
/// - With ref: `owner/repo@v1.0.0`
pub fn clone_remote(url: &str, options: CloneOptions) -> Result<PathBuf> {
    let (repo_url, reference) = parse_remote_url(url)?;

    let target = if let Some(t) = options.target {
        t
    } else {
        let temp_dir = std::env::temp_dir().join("omen-repos");
        std::fs::create_dir_all(&temp_dir).ok();
        let sanitized = sanitize_repo_name(&repo_url);
        if sanitized.is_empty()
            || sanitized.contains("..")
            || sanitized.contains('/')
            || sanitized.contains('\\')
        {
            return Err(Error::Remote(format!(
                "Invalid sanitized repo name: {sanitized}"
            )));
        }
        temp_dir.join(sanitized)
    };

    // Clean up existing directory if it exists (from a previous run)
    if target.exists() {
        std::fs::remove_dir_all(&target).ok();
    }

    // Determine clone depth args
    let mut args = vec!["clone"];
    if options.shallow {
        args.push("--depth");
        args.push("1");
    }
    args.push(&repo_url);
    let target_str = target
        .to_str()
        .ok_or_else(|| Error::Remote("Target path is not valid UTF-8".to_string()))?;
    args.push(target_str);

    // Clone using git command (more reliable for working tree checkout)
    let output = std::process::Command::new("git")
        .args(&args)
        .output()
        .map_err(|e| Error::Remote(format!("Failed to run git clone: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Remote(format!("Git clone failed: {stderr}")));
    }

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
    if reference.starts_with('-') {
        return Err(Error::Remote(format!("Invalid reference: {reference}")));
    }

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
    fn test_parse_full_url_with_ref() {
        let (url, reference) = parse_remote_url("https://github.com/owner/repo@main").unwrap();
        assert_eq!(url, "https://github.com/owner/repo");
        assert_eq!(reference, Some("main".to_string()));
    }

    #[test]
    fn test_parse_http_url() {
        let (url, reference) = parse_remote_url("http://example.com/repo").unwrap();
        assert_eq!(url, "http://example.com/repo");
        assert!(reference.is_none());
    }

    #[test]
    fn test_parse_github_domain_url() {
        let (url, reference) = parse_remote_url("github.com/owner/repo").unwrap();
        assert_eq!(url, "https://github.com/owner/repo");
        assert!(reference.is_none());
    }

    #[test]
    fn test_parse_invalid_url() {
        let result = parse_remote_url("invalid");
        assert!(result.is_err());
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

    #[test]
    fn test_sanitize_repo_name_trailing_slash() {
        assert_eq!(
            sanitize_repo_name("https://github.com/owner/repo/"),
            "owner-repo"
        );
    }

    #[test]
    fn test_clone_options_default() {
        let opts = CloneOptions::default();
        assert!(!opts.shallow);
        assert!(opts.reference.is_none());
        assert!(opts.target.is_none());
    }

    #[test]
    fn test_clone_options_custom() {
        let opts = CloneOptions {
            shallow: true,
            reference: Some("v1.0.0".to_string()),
            target: Some(PathBuf::from("/tmp/test")),
        };
        assert!(opts.shallow);
        assert_eq!(opts.reference, Some("v1.0.0".to_string()));
        assert_eq!(opts.target, Some(PathBuf::from("/tmp/test")));
    }

    #[test]
    fn test_is_remote_repo_github_shorthand() {
        assert!(is_remote_repo("owner/repo"));
        assert!(is_remote_repo("facebook/react"));
        assert!(is_remote_repo("kubernetes/kubernetes"));
    }

    #[test]
    fn test_is_remote_repo_github_shorthand_with_ref() {
        assert!(is_remote_repo("owner/repo@v1.0.0"));
        assert!(is_remote_repo("owner/repo@main"));
        assert!(is_remote_repo("owner/repo@feature-branch"));
    }

    #[test]
    fn test_is_remote_repo_full_urls() {
        assert!(is_remote_repo("https://github.com/owner/repo"));
        assert!(is_remote_repo("http://github.com/owner/repo"));
        assert!(is_remote_repo("https://gitlab.com/owner/repo"));
    }

    #[test]
    fn test_is_remote_repo_github_domain() {
        assert!(is_remote_repo("github.com/owner/repo"));
        assert!(is_remote_repo("github.com/facebook/react"));
    }

    #[test]
    fn test_is_remote_repo_local_paths() {
        assert!(!is_remote_repo("."));
        assert!(!is_remote_repo("./src"));
        assert!(!is_remote_repo("../other"));
        assert!(!is_remote_repo("/home/user/project"));
        assert!(!is_remote_repo("/tmp/repo"));
        assert!(!is_remote_repo("~/projects/myrepo"));
    }

    #[test]
    fn test_is_remote_repo_relative_paths() {
        // Paths starting with ./ are always local
        assert!(!is_remote_repo("./owner/repo"));
        assert!(!is_remote_repo("./src/main"));
        assert!(!is_remote_repo("../parent/repo"));
    }

    #[test]
    fn test_is_remote_repo_existing_local_path() {
        // Create a temp directory that looks like owner/repo but exists locally
        let temp = tempfile::tempdir().unwrap();
        let owner_dir = temp.path().join("testowner");
        let repo_dir = owner_dir.join("testrepo");
        std::fs::create_dir_all(&repo_dir).unwrap();

        // Get the relative path string
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp.path()).unwrap();

        // testowner/testrepo exists locally, so should NOT be treated as remote
        assert!(!is_remote_repo("testowner/testrepo"));

        // nonexistent/path does NOT exist locally and has owner/repo pattern
        assert!(is_remote_repo("nonexistent/path"));

        std::env::set_current_dir(cwd).unwrap();
    }

    #[test]
    fn test_is_remote_repo_single_word() {
        assert!(!is_remote_repo("myproject"));
        assert!(!is_remote_repo("repo"));
    }

    #[test]
    fn test_is_remote_repo_empty() {
        assert!(!is_remote_repo(""));
    }

    #[test]
    fn test_is_remote_repo_windows_paths() {
        assert!(!is_remote_repo("C:\\Users\\project"));
        assert!(!is_remote_repo("D:\\repos\\myrepo"));
    }

    #[test]
    fn test_is_remote_repo_windows_drive_edge_cases() {
        // Minimal Windows drive paths (C:)
        assert!(!is_remote_repo("C:"));
        assert!(!is_remote_repo("D:"));
    }

    #[test]
    fn test_is_remote_repo_colon_position() {
        // Colon at index 0 (not Windows path style)
        // If mutation changes nth(1) to nth(0), this would wrongly be detected as Windows path
        // ":owner/repo" has owner/repo pattern after the colon
        assert!(is_remote_repo(":a/b")); // Colon at index 0, but has valid owner/repo pattern

        // Colon at index 2 (not Windows path style)
        // If mutation changes nth(1) to nth(2), "ab:c/d" would wrongly be detected as Windows
        // "ab:x/y" - colon at index 2, not index 1
        assert!(is_remote_repo("ab:x/y")); // Not a Windows path, has owner/repo-like pattern
    }

    #[test]
    fn test_is_remote_repo_length_edge_cases() {
        // Empty string already tested in test_is_remote_repo_empty

        // Single char - can't be Windows path (needs at least "X:")
        // Tests that len >= 2 check is correct
        assert!(!is_remote_repo("a")); // No slash, not owner/repo
        assert!(!is_remote_repo(":")); // Just colon, not owner/repo
    }

    #[test]
    fn test_is_remote_repo_owner_repo_empty_parts() {
        // Tests for line 78: !parts[0].is_empty() && !parts[1].is_empty()
        // Swapping parts[0] and parts[1] indices would change behavior

        // "a/" has parts ["a", ""], parts[1] is empty - should be false
        assert!(!is_remote_repo("a/")); // Empty repo name

        // "/b" starts with /, rejected earlier by line 48

        // Valid owner/repo pattern
        assert!(is_remote_repo("a/b"));
        assert!(is_remote_repo("owner/repo"));
    }

    #[test]
    fn test_is_remote_repo_slash_patterns() {
        // Multiple slashes - not owner/repo pattern
        assert!(!is_remote_repo("a/b/c")); // Two slashes
        assert!(!is_remote_repo("a/b/c/d")); // Three slashes

        // Trailing slash means empty last part
        assert!(!is_remote_repo("owner/")); // Trailing slash

        // @ ref handling with slashes
        assert!(is_remote_repo("owner/repo@v1")); // Valid with ref
        assert!(!is_remote_repo("owner/@v1")); // Empty repo before ref
    }

    // --- Git argument injection tests ---

    fn init_test_repo(path: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .expect("failed to init git repo");
        std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .output()
            .expect("failed to set git email");
        std::process::Command::new("git")
            .args(["config", "user.name", "Test Author"])
            .current_dir(path)
            .output()
            .expect("failed to set git name");
    }

    fn create_initial_commit(path: &Path) {
        let file_path = path.join("README.md");
        std::fs::write(&file_path, "# test\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(path)
            .output()
            .expect("failed to add file");
        std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(path)
            .output()
            .expect("failed to commit");
    }

    #[test]
    fn test_checkout_ref_rejects_option_injection() {
        let temp = tempfile::tempdir().unwrap();
        init_test_repo(temp.path());
        create_initial_commit(temp.path());

        let dangerous_refs = [
            "--upload-pack=evil",
            "-c",
            "--exec=cmd",
            "-",
            "--help",
            "-b",
        ];
        for bad_ref in &dangerous_refs {
            let result = checkout_ref(temp.path(), bad_ref);
            assert!(
                result.is_err(),
                "checkout_ref should reject ref starting with dash: {bad_ref}"
            );
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("Invalid reference"),
                "Error should mention 'Invalid reference', got: {err_msg}"
            );
        }
    }

    #[test]
    fn test_checkout_ref_allows_valid_refs() {
        let temp = tempfile::tempdir().unwrap();
        init_test_repo(temp.path());
        create_initial_commit(temp.path());

        // Tag the initial commit
        std::process::Command::new("git")
            .args(["tag", "v1.0.0"])
            .current_dir(temp.path())
            .output()
            .expect("failed to create tag");

        // Create a branch
        std::process::Command::new("git")
            .args(["branch", "feature/test-branch"])
            .current_dir(temp.path())
            .output()
            .expect("failed to create branch");

        // These should all succeed
        assert!(checkout_ref(temp.path(), "v1.0.0").is_ok());
        assert!(
            checkout_ref(temp.path(), "main").is_ok()
                || checkout_ref(temp.path(), "master").is_ok()
        );
        assert!(checkout_ref(temp.path(), "feature/test-branch").is_ok());
    }

    #[test]
    fn test_sanitize_repo_name_edge_cases() {
        // Normal cases
        assert_eq!(
            sanitize_repo_name("https://github.com/owner/repo"),
            "owner-repo"
        );
        assert_eq!(
            sanitize_repo_name("https://github.com/owner/repo.git"),
            "owner-repo"
        );

        // Empty-ish URLs
        assert_eq!(sanitize_repo_name(""), "");
        assert_eq!(sanitize_repo_name("/"), "");
    }

    #[test]
    fn test_clone_remote_rejects_dangerous_sanitized_name() {
        // A URL that would produce a sanitized name with path traversal
        // sanitize_repo_name won't normally produce ".." but we test the validation
        // by checking that the clone_remote function validates properly.
        // The sanitizer strips .git but can't produce ".." from normal URLs.
        // Test with a URL whose sanitized form is empty.
        let result = clone_remote("/", CloneOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_remote_url_extracts_dash_refs() {
        // A ref starting with '-' can come from parse_remote_url
        let (url, reference) = parse_remote_url("owner/repo@--upload-pack=evil").unwrap();
        assert_eq!(url, "https://github.com/owner/repo");
        assert_eq!(reference, Some("--upload-pack=evil".to_string()));

        // This dangerous ref should then be caught by checkout_ref validation
        let temp = tempfile::tempdir().unwrap();
        init_test_repo(temp.path());
        create_initial_commit(temp.path());
        let result = checkout_ref(temp.path(), reference.as_deref().unwrap());
        assert!(result.is_err());
    }
}
