package vcs

import (
	"bytes"
	"errors"
	"os/exec"
	"sort"
	"strings"
	"time"
)

// ErrDirtyWorkingDir is returned when the working directory has uncommitted changes.
var ErrDirtyWorkingDir = errors.New("working directory has uncommitted changes")

// CommitInfo represents a commit with its SHA and timestamp.
type CommitInfo struct {
	SHA  string
	Date time.Time
}

// IsDirty returns true if there are uncommitted changes in the working directory.
func IsDirty(repoPath string) (bool, error) {
	cmd := exec.Command("git", "status", "--porcelain")
	cmd.Dir = repoPath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return false, errors.New(stderr.String())
	}

	return strings.TrimSpace(stdout.String()) != "", nil
}

// GetCurrentRef returns the current branch name or commit SHA (for detached HEAD).
func GetCurrentRef(repoPath string) (string, error) {
	// Try to get branch name first
	cmd := exec.Command("git", "symbolic-ref", "--short", "HEAD")
	cmd.Dir = repoPath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err == nil {
		return strings.TrimSpace(stdout.String()), nil
	}

	// Detached HEAD - get the commit SHA
	cmd = exec.Command("git", "rev-parse", "HEAD")
	cmd.Dir = repoPath
	stdout.Reset()
	stderr.Reset()
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return "", errors.New(stderr.String())
	}

	return strings.TrimSpace(stdout.String()), nil
}

// CheckoutCommit checks out a specific commit or ref.
func CheckoutCommit(repoPath, ref string) error {
	cmd := exec.Command("git", "checkout", ref)
	cmd.Dir = repoPath

	var stderr bytes.Buffer
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return errors.New(stderr.String())
	}

	return nil
}

// FindCommitsAtIntervals finds commits at regular intervals going back from now.
// If snap is true, intervals are aligned to period boundaries (1st of month, Monday).
func FindCommitsAtIntervals(repoPath string, period string, since time.Duration, snap bool) ([]CommitInfo, error) {
	// Get all commits in the time range
	sinceTime := time.Now().Add(-since)

	cmd := exec.Command("git", "log", "--format=%H %aI", "--since="+sinceTime.Format(time.RFC3339))
	cmd.Dir = repoPath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		return nil, errors.New(stderr.String())
	}

	// Parse commits
	var commits []CommitInfo
	for _, line := range strings.Split(strings.TrimSpace(stdout.String()), "\n") {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, " ", 2)
		if len(parts) != 2 {
			continue
		}
		date, err := time.Parse(time.RFC3339, parts[1])
		if err != nil {
			continue
		}
		commits = append(commits, CommitInfo{SHA: parts[0], Date: date})
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
// Commits are assumed to be sorted newest-first.
func findCommitAtOrAfter(commits []CommitInfo, target time.Time) *CommitInfo {
	// Sort by date ascending for easier searching
	sorted := make([]CommitInfo, len(commits))
	copy(sorted, commits)
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].Date.Before(sorted[j].Date)
	})

	// Find first commit on or after target
	for i := range sorted {
		if !sorted[i].Date.Before(target) {
			return &sorted[i]
		}
	}

	return nil
}
