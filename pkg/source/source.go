package source

import (
	"os"
	"sync"

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
// It is safe for concurrent use by multiple goroutines.
type TreeSource struct {
	tree vcs.Tree
	mu   sync.Mutex
}

// NewTree creates a source that reads from a git tree.
func NewTree(tree vcs.Tree) *TreeSource {
	return &TreeSource{tree: tree}
}

// Read implements ContentSource.
// It is safe for concurrent use.
func (t *TreeSource) Read(path string) ([]byte, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.tree.File(path)
}
