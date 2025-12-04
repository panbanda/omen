package complexity

import (
	"testing"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/source"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestAnalyzeFileFromSource(t *testing.T) {
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen("../../..")
	require.NoError(t, err)

	head, err := repo.Head()
	require.NoError(t, err)

	commit, err := repo.CommitObject(head.Hash())
	require.NoError(t, err)

	tree, err := commit.Tree()
	require.NoError(t, err)

	src := source.NewTree(tree)

	a := New()
	defer a.Close()

	// Analyze complexity.go from the tree
	result, err := a.AnalyzeFileFromSource(src, "pkg/analyzer/complexity/complexity.go")
	require.NoError(t, err)

	assert.Equal(t, "pkg/analyzer/complexity/complexity.go", result.Path)
	assert.Greater(t, len(result.Functions), 0)
	assert.Greater(t, result.TotalCyclomatic, uint32(0))
}
