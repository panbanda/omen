package fileproc

import (
	"context"
	"testing"

	"github.com/panbanda/omen/internal/testutil"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestMapSourceFiles(t *testing.T) {
	repoRoot := testutil.RepoRoot(t)
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen(repoRoot)
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

func TestMapSourceFilesPooled(t *testing.T) {
	repoRoot := testutil.RepoRoot(t)
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen(repoRoot)
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
		if len(goFiles) >= 10 {
			break
		}
		if parser.DetectLanguage(e.Path) == parser.LangGo {
			goFiles = append(goFiles, e.Path)
		}
	}
	require.Greater(t, len(goFiles), 0)

	// Process files from the tree using the pooled version
	src := &mockTreeSource{tree: tree}
	results := MapSourceFilesPooled(context.Background(), goFiles, src, 0, func(psr *parser.Parser, path string, content []byte) (int, error) {
		result, err := psr.Parse(content, parser.LangGo, path)
		if err != nil {
			return 0, err
		}
		return len(parser.GetFunctions(result)), nil
	})

	// Should have processed files and results should be indexed
	assert.Equal(t, len(goFiles), len(results))
}

func TestMapSourceFilesPooled_Empty(t *testing.T) {
	results := MapSourceFilesPooled[int](context.Background(), nil, nil, 0, func(psr *parser.Parser, path string, content []byte) (int, error) {
		return 1, nil
	})
	assert.Nil(t, results)
}

func TestMapSourceFilesPooled_SizeLimit(t *testing.T) {
	// Create a mock source with some content
	src := &mockMapSource{
		files: map[string][]byte{
			"small.go": []byte("package main"),
			"large.go": []byte("package main\n" + string(make([]byte, 1000))),
		},
	}

	// Only files under 100 bytes should be processed
	results := MapSourceFilesPooled(context.Background(), []string{"small.go", "large.go"}, src, 100, func(psr *parser.Parser, path string, content []byte) (string, error) {
		return path, nil
	})

	// Only 1 file should be processed (small.go)
	nonEmpty := 0
	for _, r := range results {
		if r != "" {
			nonEmpty++
		}
	}
	assert.Equal(t, 1, nonEmpty)
}

type mockMapSource struct {
	files map[string][]byte
}

func (m *mockMapSource) Read(path string) ([]byte, error) {
	content, ok := m.files[path]
	if !ok {
		return nil, assert.AnError
	}
	return content, nil
}
