package fileproc

import (
	"context"
	"testing"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMapSourceFiles(t *testing.T) {
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen("../..")
	require.NoError(t, err)

	head, err := repo.Head()
	require.NoError(t, err)

	commit, err := repo.CommitObject(head.Hash())
	require.NoError(t, err)

	tree, err := commit.Tree()
	require.NoError(t, err)

	// Get some Go files from the tree
	entries, err := tree.Entries()
	require.NoError(t, err)

	var goFiles []string
	for _, e := range entries {
		if len(goFiles) >= 5 {
			break
		}
		if parser.DetectLanguage(e.Path) == parser.LangGo {
			goFiles = append(goFiles, e.Path)
		}
	}
	require.Greater(t, len(goFiles), 0)

	// Process files from the tree using a mock source
	src := &mockTreeSource{tree: tree}
	results := MapSourceFiles(context.Background(), goFiles, src, func(psr *parser.Parser, path string, content []byte) (int, error) {
		result, err := psr.Parse(content, parser.LangGo, path)
		if err != nil {
			return 0, err
		}
		return len(parser.GetFunctions(result)), nil
	})

	// Should have processed files
	assert.Greater(t, len(results), 0)
}

// mockTreeSource implements ContentSource for testing
type mockTreeSource struct {
	tree vcs.Tree
}

func (m *mockTreeSource) Read(path string) ([]byte, error) {
	return m.tree.File(path)
}
