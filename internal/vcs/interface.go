// Package vcs provides version control system abstractions.
package vcs

import (
	"context"
	"time"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
)

// Repository provides access to git repository operations.
type Repository interface {
	// Head returns a reference to the HEAD commit.
	Head() (Reference, error)
	// Log returns a commit iterator starting from HEAD.
	Log(opts *LogOptions) (CommitIterator, error)
	// CommitObject returns the commit with the given hash.
	CommitObject(hash plumbing.Hash) (Commit, error)
	// Blame returns blame information for a file at a specific commit.
	Blame(commit Commit, path string) (*BlameResult, error)
	// BlameAtHead returns blame information for a file at HEAD using native git.
	// This is much faster than Blame() for large repositories.
	BlameAtHead(path string) (*BlameResult, error)
	// RepoPath returns the root path of the repository.
	RepoPath() string
}

// Reference represents a git reference (branch, tag, HEAD).
type Reference interface {
	Hash() plumbing.Hash
}

// LogOptions configures the commit log query.
type LogOptions struct {
	Since *time.Time
}

// CommitIterator iterates over commits.
type CommitIterator interface {
	ForEach(fn func(Commit) error) error
	Close()
}

// Commit represents a git commit.
type Commit interface {
	// Hash returns the commit hash.
	Hash() plumbing.Hash
	// NumParents returns the number of parent commits.
	NumParents() int
	// Parent returns the nth parent commit.
	Parent(n int) (Commit, error)
	// Tree returns the tree object for this commit.
	Tree() (Tree, error)
	// Stats returns file stats for this commit.
	Stats() (object.FileStats, error)
	// Author returns commit author information.
	Author() object.Signature
	// Message returns the commit message.
	Message() string
}

// TreeEntry represents a file or directory in a git tree.
type TreeEntry struct {
	Path  string
	Size  int64
	IsDir bool
}

// Tree represents a git tree object.
type Tree interface {
	// Diff computes differences between this tree and another.
	Diff(to Tree) (Changes, error)
	// Entries returns all files in the tree (recursively).
	Entries() ([]TreeEntry, error)
}

// Changes represents a collection of file changes between trees.
type Changes []Change

// Change represents a single file change.
type Change interface {
	// From returns the source file name (empty for new files).
	FromName() string
	// To returns the destination file name (empty for deleted files).
	ToName() string
	// Patch computes the patch for this change.
	Patch() (Patch, error)
}

// Patch represents a diff patch.
type Patch interface {
	FilePatches() []FilePatch
}

// FilePatch represents changes to a single file.
type FilePatch interface {
	Chunks() []Chunk
}

// Chunk represents a chunk of changes within a file patch.
type Chunk interface {
	Type() ChunkType
	Content() string
}

// ChunkType represents the type of change in a chunk.
type ChunkType int

const (
	ChunkEqual ChunkType = iota
	ChunkAdd
	ChunkDelete
)

// BlameResult contains blame information for a file.
type BlameResult struct {
	Lines []BlameLine
}

// BlameLine represents a single line in a blame result.
type BlameLine struct {
	Author     string
	AuthorName string
	Text       string
}

// Opener opens git repositories.
type Opener interface {
	// PlainOpen opens an existing git repository.
	PlainOpen(path string) (Repository, error)
	// PlainOpenWithDetect opens a git repository, detecting .git in parent directories.
	PlainOpenWithDetect(path string) (Repository, error)
}

// ContextAwareRepository extends Repository with context-aware operations.
type ContextAwareRepository interface {
	Repository
	// LogWithContext returns a commit iterator with context support.
	LogWithContext(ctx context.Context, opts *LogOptions) (CommitIterator, error)
}
