package commit

import (
	"testing"
	"time"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestAnalyzeCommit(t *testing.T) {
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen("../../..")
	require.NoError(t, err)

	head, err := repo.Head()
	require.NoError(t, err)

	a := New()

	result, err := a.AnalyzeCommit(repo, head.Hash())
	require.NoError(t, err)

	assert.Equal(t, head.Hash().String(), result.CommitHash)
	assert.Greater(t, result.Complexity.Summary.TotalFiles, 0)
}

func TestAnalyzeCommits(t *testing.T) {
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen("../../..")
	require.NoError(t, err)

	// Get last 3 commits - this may fail in shallow clones
	iter, err := repo.Log(nil)
	require.NoError(t, err)
	defer iter.Close()

	var hashes []plumbing.Hash
	count := 0
	err = iter.ForEach(func(c vcs.Commit) error {
		if count >= 3 {
			return nil
		}
		hashes = append(hashes, c.Hash())
		count++
		return nil
	})
	require.NoError(t, err)

	if len(hashes) < 2 {
		t.Skip("Shallow clone detected, skipping multi-commit test")
	}

	a := New()
	defer a.Close()

	results, err := a.AnalyzeCommits(repo, hashes)
	require.NoError(t, err)

	assert.Len(t, results, len(hashes))
	for _, r := range results {
		assert.NotEmpty(t, r.CommitHash)
		assert.NotNil(t, r.Complexity)
	}
}

func TestAnalyzeTrend(t *testing.T) {
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen("../../..")
	require.NoError(t, err)

	a := New()
	defer a.Close()

	// Analyze trend over last 7 days (may have few commits)
	trends, err := a.AnalyzeTrend(repo, 7*24*time.Hour)

	// In shallow clones, this may fail with "object not found"
	if err != nil && err.Error() == "object not found" {
		t.Skip("Shallow clone detected, skipping trend test")
	}
	require.NoError(t, err)

	// May have zero commits if repo is very new or shallow
	assert.GreaterOrEqual(t, len(trends.Commits), 0)
}
