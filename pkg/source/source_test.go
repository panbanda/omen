package source

import (
	"testing"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestFilesystemSource(t *testing.T) {
	src := NewFilesystem()

	// Read a file that exists
	content, err := src.Read("../../go.mod")
	require.NoError(t, err)
	assert.Contains(t, string(content), "module github.com/panbanda/omen")

	// Non-existent file should error
	_, err = src.Read("nonexistent.txt")
	assert.Error(t, err)
}

func TestTreeSource(t *testing.T) {
	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpen("../..")
	require.NoError(t, err)

	head, err := repo.Head()
	require.NoError(t, err)

	commit, err := repo.CommitObject(head.Hash())
	require.NoError(t, err)

	tree, err := commit.Tree()
	require.NoError(t, err)

	src := NewTree(tree)

	content, err := src.Read("go.mod")
	require.NoError(t, err)
	assert.Contains(t, string(content), "module github.com/panbanda/omen")
}
