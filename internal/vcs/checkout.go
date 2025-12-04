package vcs

import (
	"errors"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
)

// ErrDirtyWorkingDir is returned when the working directory has uncommitted changes.
var ErrDirtyWorkingDir = errors.New("working directory has uncommitted changes")

// ErrDetachedHead is returned when the repository is in detached HEAD state.
var ErrDetachedHead = errors.New("repository is in detached HEAD state; checkout a branch first")

// CommitInfo represents a commit with its SHA and timestamp.
type CommitInfo struct {
	SHA  string
	Date time.Time
}

// IsDirty returns true if there are uncommitted changes in the working directory.
// Untracked files are not considered dirty.
func IsDirty(repoPath string) (bool, error) {
	repo, err := git.PlainOpen(repoPath)
	if err != nil {
		return false, err
	}

	wt, err := repo.Worktree()
	if err != nil {
		return false, err
	}

	status, err := wt.Status()
	if err != nil {
		return false, err
	}

	for _, s := range status {
		// Skip untracked files
		if s.Staging == git.Untracked && s.Worktree == git.Untracked {
			continue
		}
		// Any staged or modified file means dirty
		if s.Staging != git.Unmodified || s.Worktree != git.Unmodified {
			return true, nil
		}
	}

	return false, nil
}

// GetCurrentRef returns the current branch name or commit SHA (for detached HEAD).
func GetCurrentRef(repoPath string) (string, error) {
	repo, err := git.PlainOpen(repoPath)
	if err != nil {
		return "", err
	}

	head, err := repo.Head()
	if err != nil {
		return "", err
	}

	// If it's a branch, return the short name
	if head.Name().IsBranch() {
		return head.Name().Short(), nil
	}

	// Detached HEAD - return the commit SHA
	return head.Hash().String(), nil
}

// IsDetachedHead returns true if the repository is in detached HEAD state.
func IsDetachedHead(repoPath string) (bool, error) {
	repo, err := git.PlainOpen(repoPath)
	if err != nil {
		return false, err
	}

	head, err := repo.Head()
	if err != nil {
		return false, err
	}

	return !head.Name().IsBranch(), nil
}

// CheckoutCommit checks out a specific commit or ref.
func CheckoutCommit(repoPath, ref string) error {
	repo, err := git.PlainOpen(repoPath)
	if err != nil {
		return err
	}

	wt, err := repo.Worktree()
	if err != nil {
		return err
	}

	// Try to resolve as a branch first
	branchRef := plumbing.NewBranchReferenceName(ref)
	if _, err := repo.Reference(branchRef, true); err == nil {
		return wt.Checkout(&git.CheckoutOptions{
			Branch: branchRef,
		})
	}

	// Try to resolve as a commit hash
	hash, err := repo.ResolveRevision(plumbing.Revision(ref))
	if err != nil {
		return err
	}

	return wt.Checkout(&git.CheckoutOptions{
		Hash: *hash,
	})
}

// FindCommitsAtIntervals finds commits at regular intervals going back from now.
// If snap is true, intervals are aligned to period boundaries (1st of month, Monday).
func FindCommitsAtIntervals(repoPath string, period string, since time.Duration, snap bool) ([]CommitInfo, error) {
	repo, err := git.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	sinceTime := time.Now().Add(-since)

	// Get commits since the start time
	iter, err := repo.Log(&git.LogOptions{
		Since: &sinceTime,
	})
	if err != nil {
		return nil, err
	}
	defer iter.Close()

	// Collect all commits
	var commits []CommitInfo
	err = iter.ForEach(func(c *object.Commit) error {
		commits = append(commits, CommitInfo{
			SHA:  c.Hash.String(),
			Date: c.Author.When,
		})
		return nil
	})
	if err != nil {
		return nil, err
	}

	if len(commits) == 0 {
		return nil, nil
	}

	// Generate interval boundaries
	boundaries := generateBoundaries(period, since, snap)

	// Find the first commit on or after each boundary
	var result []CommitInfo
	for _, boundary := range boundaries {
		commit := findCommitAtOrAfter(commits, boundary)
		if commit != nil {
			// Avoid duplicates
			if len(result) == 0 || result[len(result)-1].SHA != commit.SHA {
				result = append(result, *commit)
			}
		}
	}

	return result, nil
}

// generateBoundaries generates time boundaries for sampling.
func generateBoundaries(period string, since time.Duration, snap bool) []time.Time {
	now := time.Now()
	start := now.Add(-since)

	var boundaries []time.Time

	if snap {
		// Snap to period boundaries
		switch period {
		case "weekly":
			// Start from the Monday of the week containing 'start'
			current := startOfWeek(start)
			for current.Before(now) {
				boundaries = append(boundaries, current)
				current = current.AddDate(0, 0, 7)
			}
		case "monthly":
			// Start from the 1st of the month containing 'start'
			current := startOfMonth(start)
			for current.Before(now) {
				boundaries = append(boundaries, current)
				current = current.AddDate(0, 1, 0)
			}
		}
	} else {
		// Regular intervals from now going back
		var interval time.Duration
		switch period {
		case "weekly":
			interval = 7 * 24 * time.Hour
		case "monthly":
			interval = 30 * 24 * time.Hour
		}

		current := start
		for current.Before(now) {
			boundaries = append(boundaries, current)
			current = current.Add(interval)
		}
	}

	return boundaries
}

// startOfWeek returns the Monday of the week containing t.
func startOfWeek(t time.Time) time.Time {
	weekday := int(t.Weekday())
	if weekday == 0 {
		weekday = 7 // Sunday
	}
	daysBack := weekday - 1 // Monday is 1
	return time.Date(t.Year(), t.Month(), t.Day()-daysBack, 0, 0, 0, 0, t.Location())
}

// startOfMonth returns the first day of the month containing t.
func startOfMonth(t time.Time) time.Time {
	return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, t.Location())
}

// findCommitAtOrAfter finds the first commit on or after the given time.
// Commits are assumed to be sorted newest-first (from git log).
func findCommitAtOrAfter(commits []CommitInfo, target time.Time) *CommitInfo {
	// Search from oldest to newest (reverse order since commits are newest-first)
	var result *CommitInfo
	for i := len(commits) - 1; i >= 0; i-- {
		if !commits[i].Date.Before(target) {
			result = &commits[i]
			break
		}
	}
	return result
}
