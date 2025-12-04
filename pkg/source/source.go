package source

import (
	"os"

	"github.com/panbanda/omen/internal/vcs"
)

// ContentSource provides file content from a specific source.
type ContentSource interface {
	// Read returns the content of the file at path.
	Read(path string) ([]byte, error)
}

// FilesystemSource reads files from the local filesystem.
type FilesystemSource struct{}

// NewFilesystem creates a source that reads from the filesystem.
func NewFilesystem() *FilesystemSource {
	return &FilesystemSource{}
}

// Read implements ContentSource.
func (f *FilesystemSource) Read(path string) ([]byte, error) {
	return os.ReadFile(path)
}

// TreeSource reads files from a git tree.
type TreeSource struct {
	tree vcs.Tree
}

// NewTree creates a source that reads from a git tree.
func NewTree(tree vcs.Tree) *TreeSource {
	return &TreeSource{tree: tree}
}

// Read implements ContentSource.
func (t *TreeSource) Read(path string) ([]byte, error) {
	return t.tree.File(path)
}
