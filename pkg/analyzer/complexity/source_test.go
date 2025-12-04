package complexity

import (
	"testing"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/parser"
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

func TestAnalyzeProjectFromSource(t *testing.T) {
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

	// Get Go files from tree
	entries, err := tree.Entries()
	require.NoError(t, err)

	var goFiles []string
	for _, e := range entries {
		if parser.DetectLanguage(e.Path) == parser.LangGo && !e.IsDir {
			goFiles = append(goFiles, e.Path)
		}
	}
	require.Greater(t, len(goFiles), 0)

	a := New()
	defer a.Close()

	analysis, err := a.AnalyzeProjectFromSource(goFiles, src)
	require.NoError(t, err)

	assert.Greater(t, analysis.Summary.TotalFiles, 0)
	assert.Greater(t, analysis.Summary.TotalFunctions, 0)
}
