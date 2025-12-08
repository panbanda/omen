package vcs

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/go-git/go-git/v6"
	"github.com/go-git/go-git/v6/plumbing/object"
)

func TestNewGitOpener(t *testing.T) {
	opener := NewGitOpener()
	if opener == nil {
		t.Fatal("NewGitOpener() returned nil")
	}
}

func TestGitOpener_PlainOpen(t *testing.T) {
	repoPath := initTestRepo(t)

	opener := NewGitOpener()
	repo, err := opener.PlainOpen(repoPath)
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}
	if repo == nil {
		t.Fatal("PlainOpen() returned nil repository")
	}
}

func TestGitOpener_PlainOpen_NonExistent(t *testing.T) {
	opener := NewGitOpener()
	_, err := opener.PlainOpen("/nonexistent/path")
	if err == nil {
		t.Error("PlainOpen() should return error for non-existent path")
	}
}

func TestGitOpener_PlainOpenWithDetect(t *testing.T) {
	repoPath := initTestRepo(t)

	// Create a subdirectory
	subDir := filepath.Join(repoPath, "subdir")
	if err := os.MkdirAll(subDir, 0755); err != nil {
		t.Fatal(err)
	}

	opener := NewGitOpener()
	repo, err := opener.PlainOpenWithDetect(subDir)
	if err != nil {
		t.Fatalf("PlainOpenWithDetect() error = %v", err)
	}
	if repo == nil {
		t.Fatal("PlainOpenWithDetect() returned nil repository")
	}
}

func TestGitRepository_Head(t *testing.T) {
	repoPath := initTestRepoWithCommit(t)

	opener := NewGitOpener()
	repo, err := opener.PlainOpen(repoPath)
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	head, err := repo.Head()
	if err != nil {
		t.Fatalf("Head() error = %v", err)
	}
	if head == nil {
		t.Fatal("Head() returned nil")
	}

	hash := head.Hash()
	if hash.IsZero() {
		t.Error("Hash() returned zero hash")
	}
}

func TestGitRepository_Log(t *testing.T) {
	repoPath := initTestRepoWithCommit(t)

	opener := NewGitOpener()
	repo, err := opener.PlainOpen(repoPath)
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	iter, err := repo.Log(nil)
	if err != nil {
		t.Fatalf("Log() error = %v", err)
	}
	defer iter.Close()

	commitCount := 0
	err = iter.ForEach(func(c Commit) error {
		commitCount++
		return nil
	})
	if err != nil {
		t.Fatalf("ForEach() error = %v", err)
	}
	if commitCount == 0 {
		t.Error("Expected at least 1 commit")
	}
}

func TestGitRepository_Log_WithSince(t *testing.T) {
	repoPath := initTestRepoWithCommit(t)

	opener := NewGitOpener()
	repo, err := opener.PlainOpen(repoPath)
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	since := time.Now().AddDate(0, 0, -1)
	iter, err := repo.Log(&LogOptions{Since: &since})
	if err != nil {
		t.Fatalf("Log() error = %v", err)
	}
	defer iter.Close()

	commitCount := 0
	err = iter.ForEach(func(c Commit) error {
		commitCount++
		return nil
	})
	if err != nil {
		t.Fatalf("ForEach() error = %v", err)
	}
	if commitCount != 1 {
		t.Errorf("Expected 1 commit within last day, got %d", commitCount)
	}
}

func TestGitRepository_CommitObject(t *testing.T) {
	repoPath := initTestRepoWithCommit(t)

	opener := NewGitOpener()
	repo, err := opener.PlainOpen(repoPath)
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	head, err := repo.Head()
	if err != nil {
		t.Fatalf("Head() error = %v", err)
	}

	commit, err := repo.CommitObject(head.Hash())
	if err != nil {
		t.Fatalf("CommitObject() error = %v", err)
	}
	if commit == nil {
		t.Fatal("CommitObject() returned nil")
	}
	if commit.Hash() != head.Hash() {
		t.Error("Commit hash doesn't match head hash")
	}
}

func TestGitCommit_Methods(t *testing.T) {
	repoPath := initTestRepoWithMultipleCommits(t)

	opener := NewGitOpener()
	repo, err := opener.PlainOpen(repoPath)
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	head, _ := repo.Head()
	commit, _ := repo.CommitObject(head.Hash())

	// Test NumParents
	if commit.NumParents() != 1 {
		t.Errorf("NumParents() = %d, want 1", commit.NumParents())
	}

	// Test Parent
	parent, err := commit.Parent(0)
	if err != nil {
		t.Fatalf("Parent() error = %v", err)
	}
	if parent == nil {
		t.Fatal("Parent() returned nil")
	}

	// Test Tree
	tree, err := commit.Tree()
	if err != nil {
		t.Fatalf("Tree() error = %v", err)
	}
	if tree == nil {
		t.Fatal("Tree() returned nil")
	}

	// Test Stats
	stats, err := commit.Stats()
	if err != nil {
		t.Fatalf("Stats() error = %v", err)
	}
	if len(stats) == 0 {
		t.Error("Stats() returned empty slice")
	}

	// Test Author
	author := commit.Author()
	if author.Name == "" {
		t.Error("Author name should not be empty")
	}
}

func TestGitTree_Diff(t *testing.T) {
	repoPath := initTestRepoWithMultipleCommits(t)

	opener := NewGitOpener()
	repo, _ := opener.PlainOpen(repoPath)
	head, _ := repo.Head()
	commit, _ := repo.CommitObject(head.Hash())

	tree, _ := commit.Tree()
	parent, _ := commit.Parent(0)
	parentTree, _ := parent.Tree()

	changes, err := parentTree.Diff(tree)
	if err != nil {
		t.Fatalf("Diff() error = %v", err)
	}
	if len(changes) == 0 {
		t.Error("Expected at least 1 change")
	}
}

func TestGitChange_Methods(t *testing.T) {
	repoPath := initTestRepoWithMultipleCommits(t)

	opener := NewGitOpener()
	repo, _ := opener.PlainOpen(repoPath)
	head, _ := repo.Head()
	commit, _ := repo.CommitObject(head.Hash())
	tree, _ := commit.Tree()
	parent, _ := commit.Parent(0)
	parentTree, _ := parent.Tree()
	changes, _ := parentTree.Diff(tree)

	if len(changes) == 0 {
		t.Fatal("No changes to test")
	}

	change := changes[0]

	// Test ToName/FromName
	toName := change.ToName()
	fromName := change.FromName()
	if toName == "" && fromName == "" {
		t.Error("Both ToName and FromName are empty")
	}

	// Test Patch
	patch, err := change.Patch()
	if err != nil {
		t.Fatalf("Patch() error = %v", err)
	}
	if patch == nil {
		t.Fatal("Patch() returned nil")
	}

	// Test FilePatches
	filePatches := patch.FilePatches()
	if len(filePatches) == 0 {
		t.Error("Expected at least 1 file patch")
	}
}

func TestGitChunk_Methods(t *testing.T) {
	repoPath := initTestRepoWithMultipleCommits(t)

	opener := NewGitOpener()
	repo, _ := opener.PlainOpen(repoPath)
	head, _ := repo.Head()
	commit, _ := repo.CommitObject(head.Hash())
	tree, _ := commit.Tree()
	parent, _ := commit.Parent(0)
	parentTree, _ := parent.Tree()
	changes, _ := parentTree.Diff(tree)

	if len(changes) == 0 {
		t.Fatal("No changes to test")
	}

	patch, _ := changes[0].Patch()
	filePatches := patch.FilePatches()
	if len(filePatches) == 0 {
		t.Fatal("No file patches to test")
	}

	chunks := filePatches[0].Chunks()
	if len(chunks) == 0 {
		t.Fatal("No chunks to test")
	}

	chunk := chunks[0]
	chunkType := chunk.Type()
	content := chunk.Content()

	// Just verify they don't panic and return something reasonable
	if chunkType != ChunkEqual && chunkType != ChunkAdd && chunkType != ChunkDelete {
		t.Errorf("Unexpected chunk type: %d", chunkType)
	}
	if content == "" && chunkType != ChunkEqual {
		t.Error("Non-equal chunk has empty content")
	}
}

func TestDefaultOpener(t *testing.T) {
	opener := DefaultOpener()
	if opener == nil {
		t.Fatal("DefaultOpener() returned nil")
	}
}

func TestSetDefaultOpener(t *testing.T) {
	original := DefaultOpener()
	defer SetDefaultOpener(original)

	newOpener := NewGitOpener()
	SetDefaultOpener(newOpener)

	if DefaultOpener() != newOpener {
		t.Error("SetDefaultOpener() didn't change default opener")
	}
}

func TestErrInvalidType(t *testing.T) {
	if ErrInvalidType.Error() == "" {
		t.Error("ErrInvalidType should have non-empty message")
	}
}

func TestChunkTypes(t *testing.T) {
	if ChunkEqual >= ChunkAdd || ChunkAdd >= ChunkDelete {
		t.Error("Chunk type constants should be in order: Equal < Add < Delete")
	}
}

// Helper functions

func initTestRepo(t *testing.T) string {
	t.Helper()
	repoPath := t.TempDir()
	_, err := git.PlainInit(repoPath, false)
	if err != nil {
		t.Fatalf("Failed to init repo: %v", err)
	}
	return repoPath
}

func initTestRepoWithCommit(t *testing.T) string {
	t.Helper()
	repoPath := t.TempDir()
	repo, err := git.PlainInit(repoPath, false)
	if err != nil {
		t.Fatalf("Failed to init repo: %v", err)
	}

	// Create and commit a file
	testFile := filepath.Join(repoPath, "test.txt")
	if err := os.WriteFile(testFile, []byte("initial content\n"), 0644); err != nil {
		t.Fatal(err)
	}

	w, _ := repo.Worktree()
	w.Add("test.txt")
	_, err = w.Commit("Initial commit", &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test",
			Email: "test@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	return repoPath
}

func initTestRepoWithMultipleCommits(t *testing.T) string {
	t.Helper()
	repoPath := t.TempDir()
	repo, err := git.PlainInit(repoPath, false)
	if err != nil {
		t.Fatalf("Failed to init repo: %v", err)
	}

	w, _ := repo.Worktree()

	// First commit
	testFile := filepath.Join(repoPath, "test.txt")
	if err := os.WriteFile(testFile, []byte("initial content\n"), 0644); err != nil {
		t.Fatal(err)
	}
	w.Add("test.txt")
	_, err = w.Commit("Initial commit", &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test",
			Email: "test@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	// Second commit with modifications
	if err := os.WriteFile(testFile, []byte("modified content\nmore lines\n"), 0644); err != nil {
		t.Fatal(err)
	}
	w.Add("test.txt")
	_, err = w.Commit("Second commit", &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test",
			Email: "test@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	return repoPath
}

func TestTreeEntries(t *testing.T) {
	// Use the existing omen repo as a fixture
	opener := NewGitOpener()
	repo, err := opener.PlainOpen("../..")
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	head, err := repo.Head()
	if err != nil {
		t.Fatalf("Head() error = %v", err)
	}

	commit, err := repo.CommitObject(head.Hash())
	if err != nil {
		t.Fatalf("CommitObject() error = %v", err)
	}

	tree, err := commit.Tree()
	if err != nil {
		t.Fatalf("Tree() error = %v", err)
	}

	entries, err := tree.Entries()
	if err != nil {
		t.Fatalf("Entries() error = %v", err)
	}

	// Should have entries
	if len(entries) == 0 {
		t.Error("Entries() returned empty slice")
	}

	// Should include known files
	var foundGoMod bool
	for _, e := range entries {
		if e.Path == "go.mod" {
			foundGoMod = true
			if e.IsDir {
				t.Error("go.mod should not be a directory")
			}
		}
	}
	if !foundGoMod {
		t.Error("should find go.mod in tree entries")
	}
}

func TestTreeFile(t *testing.T) {
	opener := NewGitOpener()
	repo, err := opener.PlainOpen("../..")
	if err != nil {
		t.Fatalf("PlainOpen() error = %v", err)
	}

	head, err := repo.Head()
	if err != nil {
		t.Fatalf("Head() error = %v", err)
	}

	commit, err := repo.CommitObject(head.Hash())
	if err != nil {
		t.Fatalf("CommitObject() error = %v", err)
	}

	tree, err := commit.Tree()
	if err != nil {
		t.Fatalf("Tree() error = %v", err)
	}

	// Read go.mod which should exist
	content, err := tree.File("go.mod")
	if err != nil {
		t.Fatalf("File() error = %v", err)
	}
	if !bytes.Contains(content, []byte("module github.com/panbanda/omen")) {
		t.Error("go.mod should contain module declaration")
	}

	// Non-existent file should error
	_, err = tree.File("nonexistent.txt")
	if err == nil {
		t.Error("File() should return error for non-existent file")
	}
}
