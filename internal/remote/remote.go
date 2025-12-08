package remote

import (
	"os"
	"strings"
)

// Source represents a remote repository to analyze.
type Source struct {
	URL      string // normalized git URL
	Ref      string // branch, tag, or SHA (empty = default branch)
	CloneDir string // temp directory after clone
}

// Parse detects if a path is a remote reference.
// Returns nil if path exists on filesystem (local path takes precedence).
func Parse(path string) (*Source, error) {
	// Check if path exists locally
	if _, err := os.Stat(path); err == nil {
		return nil, nil
	}

	// Extract ref from path@ref syntax (but not for SSH URLs with @)
	ref := ""
	if !strings.HasPrefix(path, "git@") {
		if idx := strings.LastIndex(path, "@"); idx != -1 {
			ref = path[idx+1:]
			path = path[:idx]
		}
	}

	// Check for GitHub shorthand: owner/repo
	if isGitHubShorthand(path) {
		return &Source{
			URL: "https://github.com/" + path,
			Ref: ref,
		}, nil
	}

	// Check for full URL patterns
	url := normalizeURL(path)
	if url != "" {
		return &Source{
			URL: url,
			Ref: ref,
		}, nil
	}

	return nil, nil
}

// isGitHubShorthand returns true if path matches owner/repo pattern.
func isGitHubShorthand(path string) bool {
	slashIdx := strings.Index(path, "/")
	if slashIdx == -1 {
		return false
	}
	// Must have exactly one slash
	if strings.Count(path, "/") != 1 {
		return false
	}
	// No dots before the slash (would indicate a domain)
	if strings.Contains(path[:slashIdx], ".") {
		return false
	}
	// Both parts must be non-empty
	return slashIdx > 0 && slashIdx < len(path)-1
}

// normalizeURL converts various URL formats to a cloneable URL.
func normalizeURL(path string) string {
	// Already a full URL
	if strings.HasPrefix(path, "https://") || strings.HasPrefix(path, "http://") {
		return path
	}

	// SSH URL (git@host:path)
	if strings.HasPrefix(path, "git@") {
		return path
	}

	// Domain without scheme (github.com/owner/repo)
	if strings.Contains(path, ".") && strings.Contains(path, "/") {
		return "https://" + path
	}

	return ""
}
