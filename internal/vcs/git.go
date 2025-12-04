package vcs

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/format/diff"
	"github.com/go-git/go-git/v5/plumbing/object"
)

// ErrInvalidType is returned when a type assertion fails for vcs types.
var ErrInvalidType = errors.New("invalid type")

// GitOpener opens git repositories using go-git.
type GitOpener struct{}

// NewGitOpener creates a new GitOpener.
func NewGitOpener() *GitOpener {
	return &GitOpener{}
}

// PlainOpen opens an existing git repository.
func (o *GitOpener) PlainOpen(path string) (Repository, error) {
	repo, err := git.PlainOpen(path)
	if err != nil {
		return nil, err
	}
	absPath, _ := filepath.Abs(path)
	return &gitRepository{repo: repo, repoPath: absPath}, nil
}

// PlainOpenWithDetect opens a git repository, detecting .git in parent directories.
func (o *GitOpener) PlainOpenWithDetect(path string) (Repository, error) {
	repo, err := git.PlainOpenWithOptions(path, &git.PlainOpenOptions{
		DetectDotGit: true,
	})
	if err != nil {
		return nil, err
	}
	// Find the actual repo root by looking for worktree
	wt, err := repo.Worktree()
	var repoPath string
	if err == nil {
		repoPath = wt.Filesystem.Root()
	} else {
		// Fallback to provided path
		repoPath, _ = filepath.Abs(path)
	}
	return &gitRepository{repo: repo, repoPath: repoPath}, nil
}

// gitRepository wraps go-git Repository.
type gitRepository struct {
	repo     *git.Repository
	repoPath string
}

func (r *gitRepository) Head() (Reference, error) {
	ref, err := r.repo.Head()
	if err != nil {
		return nil, err
	}
	return &gitReference{ref: ref}, nil
}

func (r *gitRepository) Log(opts *LogOptions) (CommitIterator, error) {
	gitOpts := &git.LogOptions{}
	if opts != nil && opts.Since != nil {
		gitOpts.Since = opts.Since
	}
	iter, err := r.repo.Log(gitOpts)
	if err != nil {
		return nil, err
	}
	return &gitCommitIterator{iter: iter}, nil
}

func (r *gitRepository) LogWithContext(ctx context.Context, opts *LogOptions) (CommitIterator, error) {
	return r.Log(opts)
}

func (r *gitRepository) CommitObject(hash plumbing.Hash) (Commit, error) {
	commit, err := r.repo.CommitObject(hash)
	if err != nil {
		return nil, err
	}
	return &gitCommit{commit: commit}, nil
}

func (r *gitRepository) Blame(commit Commit, path string) (*BlameResult, error) {
	gc, ok := commit.(*gitCommit)
	if !ok {
		return nil, ErrInvalidType
	}
	blame, err := git.Blame(gc.commit, path)
	if err != nil {
		return nil, err
	}
	result := &BlameResult{
		Lines: make([]BlameLine, len(blame.Lines)),
	}
	for i, line := range blame.Lines {
		result.Lines[i] = BlameLine{
			Author:     line.Author,
			AuthorName: line.AuthorName,
			Text:       line.Text,
		}
	}
	return result, nil
}

// BlameAtHead uses native git blame for better performance on large repos.
// Format: git blame --line-porcelain
func (r *gitRepository) BlameAtHead(path string) (*BlameResult, error) {
	cmd := exec.Command("git", "blame", "--line-porcelain", "HEAD", "--", path)
	cmd.Dir = r.repoPath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		// Check for "no such path" or similar - return nil for non-existent files
		if strings.Contains(stderr.String(), "no such path") ||
			strings.Contains(stderr.String(), "does not exist") {
			return nil, nil
		}
		return nil, errors.New(stderr.String())
	}

	return parseGitBlameOutput(&stdout)
}

// RepoPath returns the repository root path.
func (r *gitRepository) RepoPath() string {
	return r.repoPath
}

// parseGitBlameOutput parses git blame --line-porcelain output.
func parseGitBlameOutput(r *bytes.Buffer) (*BlameResult, error) {
	var lines []BlameLine
	scanner := bufio.NewScanner(r)

	var currentLine BlameLine
	for scanner.Scan() {
		line := scanner.Text()

		switch {
		case strings.HasPrefix(line, "author "):
			currentLine.AuthorName = strings.TrimPrefix(line, "author ")
		case strings.HasPrefix(line, "author-mail "):
			// Extract email from <email@example.com>
			email := strings.TrimPrefix(line, "author-mail ")
			email = strings.TrimPrefix(email, "<")
			email = strings.TrimSuffix(email, ">")
			currentLine.Author = email
		case strings.HasPrefix(line, "\t"):
			// Line content (prefixed with tab)
			currentLine.Text = strings.TrimPrefix(line, "\t")
			lines = append(lines, currentLine)
			currentLine = BlameLine{}
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, err
	}

	return &BlameResult{Lines: lines}, nil
}

// gitReference wraps go-git Reference.
type gitReference struct {
	ref *plumbing.Reference
}

func (r *gitReference) Hash() plumbing.Hash {
	return r.ref.Hash()
}

// gitCommitIterator wraps go-git CommitIter.
type gitCommitIterator struct {
	iter object.CommitIter
}

func (i *gitCommitIterator) ForEach(fn func(Commit) error) error {
	return i.iter.ForEach(func(c *object.Commit) error {
		return fn(&gitCommit{commit: c})
	})
}

func (i *gitCommitIterator) Close() {
	i.iter.Close()
}

// gitCommit wraps go-git Commit.
type gitCommit struct {
	commit *object.Commit
}

func (c *gitCommit) Hash() plumbing.Hash {
	return c.commit.Hash
}

func (c *gitCommit) NumParents() int {
	return c.commit.NumParents()
}

func (c *gitCommit) Parent(n int) (Commit, error) {
	parent, err := c.commit.Parent(n)
	if err != nil {
		return nil, err
	}
	return &gitCommit{commit: parent}, nil
}

func (c *gitCommit) Tree() (Tree, error) {
	tree, err := c.commit.Tree()
	if err != nil {
		return nil, err
	}
	return &gitTree{tree: tree}, nil
}

func (c *gitCommit) Stats() (object.FileStats, error) {
	return c.commit.Stats()
}

func (c *gitCommit) Author() object.Signature {
	return c.commit.Author
}

func (c *gitCommit) Message() string {
	return c.commit.Message
}

// gitTree wraps go-git Tree.
type gitTree struct {
	tree *object.Tree
}

func (t *gitTree) Diff(to Tree) (Changes, error) {
	gt, ok := to.(*gitTree)
	if !ok {
		return nil, ErrInvalidType
	}
	objChanges, err := t.tree.Diff(gt.tree)
	if err != nil {
		return nil, err
	}
	changes := make(Changes, len(objChanges))
	for i, c := range objChanges {
		changes[i] = &gitChange{change: c}
	}
	return changes, nil
}

func (t *gitTree) Entries() ([]TreeEntry, error) {
	var entries []TreeEntry
	err := t.tree.Files().ForEach(func(f *object.File) error {
		entries = append(entries, TreeEntry{
			Path:  f.Name,
			Size:  f.Size,
			IsDir: false,
		})
		return nil
	})
	if err != nil {
		return nil, err
	}
	return entries, nil
}

// gitChange wraps go-git Change.
type gitChange struct {
	change *object.Change
}

func (c *gitChange) FromName() string {
	return c.change.From.Name
}

func (c *gitChange) ToName() string {
	return c.change.To.Name
}

func (c *gitChange) Patch() (Patch, error) {
	patch, err := c.change.Patch()
	if err != nil {
		return nil, err
	}
	return &gitPatch{patch: patch}, nil
}

// gitPatch wraps go-git Patch.
type gitPatch struct {
	patch *object.Patch
}

func (p *gitPatch) FilePatches() []FilePatch {
	filePatches := p.patch.FilePatches()
	result := make([]FilePatch, len(filePatches))
	for i, fp := range filePatches {
		result[i] = &gitFilePatch{filePatch: fp}
	}
	return result
}

// gitFilePatch wraps go-git FilePatch.
type gitFilePatch struct {
	filePatch diff.FilePatch
}

func (fp *gitFilePatch) Chunks() []Chunk {
	chunks := fp.filePatch.Chunks()
	result := make([]Chunk, len(chunks))
	for i, c := range chunks {
		result[i] = &gitChunk{chunk: c}
	}
	return result
}

// gitChunk wraps go-git Chunk.
type gitChunk struct {
	chunk diff.Chunk
}

func (c *gitChunk) Type() ChunkType {
	switch c.chunk.Type() {
	case diff.Add:
		return ChunkAdd
	case diff.Delete:
		return ChunkDelete
	default:
		return ChunkEqual
	}
}

func (c *gitChunk) Content() string {
	return c.chunk.Content()
}

// Default opener singleton
var defaultOpener Opener = NewGitOpener()

// DefaultOpener returns the default git opener.
func DefaultOpener() Opener {
	return defaultOpener
}

// SetDefaultOpener sets the default git opener (useful for testing).
func SetDefaultOpener(opener Opener) {
	defaultOpener = opener
}
