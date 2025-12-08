package remote

import (
	"context"
	"fmt"
	"io"
	"os"
	"strings"

	"github.com/go-git/go-git/v6"
	"github.com/go-git/go-git/v6/plumbing"
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

// Clone fetches the repository to a temp directory.
func (s *Source) Clone(ctx context.Context, progress io.Writer, shallow bool) error {
	dir, err := os.MkdirTemp("", "omen-*")
	if err != nil {
		return fmt.Errorf("create temp dir: %w", err)
	}
	s.CloneDir = dir

	opts := &git.CloneOptions{
		URL:      s.URL,
		Progress: progress,
	}
	if shallow {
		opts.Depth = 1
	}

	repo, err := git.PlainCloneContext(ctx, dir, opts)
	if err != nil {
		os.RemoveAll(dir)
		s.CloneDir = ""
		return fmt.Errorf("clone: %w", err)
	}

	if s.Ref != "" {
		if err := checkoutRef(repo, s.Ref); err != nil {
			os.RemoveAll(dir)
			s.CloneDir = ""
			return fmt.Errorf("checkout %s: %w", s.Ref, err)
		}
	}

	return nil
}

// Cleanup removes the temp directory.
func (s *Source) Cleanup() error {
	if s.CloneDir == "" {
		return nil
	}
	err := os.RemoveAll(s.CloneDir)
	s.CloneDir = ""
	return err
}

// checkoutRef checks out a specific branch, tag, or commit.
func checkoutRef(repo *git.Repository, ref string) error {
	wt, err := repo.Worktree()
	if err != nil {
		return err
	}

	// Try as branch first
	branchRef := plumbing.NewBranchReferenceName(ref)
	if _, err := repo.Reference(branchRef, true); err == nil {
		return wt.Checkout(&git.CheckoutOptions{
			Branch: branchRef,
		})
	}

	// Try as tag
	tagRef := plumbing.NewTagReferenceName(ref)
	if _, err := repo.Reference(tagRef, true); err == nil {
		return wt.Checkout(&git.CheckoutOptions{
			Branch: tagRef,
		})
	}

	// Try to resolve as commit hash
	hash, err := repo.ResolveRevision(plumbing.Revision(ref))
	if err != nil {
		return fmt.Errorf("reference not found: %s", ref)
	}

	return wt.Checkout(&git.CheckoutOptions{
		Hash: *hash,
	})
}
