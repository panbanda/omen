package churn

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/stretchr/testify/mock"
)

func TestChurnAnalyzer_WithMockOpener(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).Return(nil)
	mockIter.EXPECT().Close().Return()

	ctx := context.Background()
	analyzer := New(WithOpener(mockOpener))
	result, err := analyzer.Analyze(ctx, "/fake/repo", nil)

	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}
	if result == nil {
		t.Fatal("Expected non-nil result")
	}
	if result.PeriodDays != 30 {
		t.Errorf("PeriodDays = %d, want 30", result.PeriodDays)
	}
}

func TestChurnAnalyzer_OpenError(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockOpener.EXPECT().PlainOpen("/invalid/path").Return(nil, errors.New("not a git repository"))

	ctx := context.Background()
	analyzer := New(WithOpener(mockOpener))
	_, err := analyzer.Analyze(ctx, "/invalid/path", nil)

	if err == nil {
		t.Fatal("Expected error for invalid path")
	}
}

func TestChurnAnalyzer_LogError(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(nil, errors.New("log error"))

	ctx := context.Background()
	analyzer := New(WithOpener(mockOpener))
	_, err := analyzer.Analyze(ctx, "/fake/repo", nil)

	if err == nil {
		t.Fatal("Expected error from Log()")
	}
}

func TestChurnAnalyzer_WithCommits(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit := mocks.NewMockCommit(t)
	mockParent := mocks.NewMockCommit(t)
	mockTree := mocks.NewMockTree(t)
	mockParentTree := mocks.NewMockTree(t)
	mockChange := mocks.NewMockChange(t)
	mockPatch := mocks.NewMockPatch(t)
	mockFilePatch := mocks.NewMockFilePatch(t)
	mockChunk := mocks.NewMockChunk(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	// Set up ForEach to call the callback with our mock commit
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		return fn(mockCommit)
	})
	mockIter.EXPECT().Close().Return()

	// Commit has 1 parent
	mockCommit.EXPECT().NumParents().Return(1)
	mockCommit.EXPECT().Parent(0).Return(mockParent, nil)
	mockCommit.EXPECT().Tree().Return(mockTree, nil)
	mockCommit.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  time.Now(),
	}).Maybe()

	mockParent.EXPECT().Tree().Return(mockParentTree, nil)

	// Set up diff
	mockParentTree.EXPECT().Diff(mockTree).Return(vcs.Changes{mockChange}, nil)

	mockChange.EXPECT().ToName().Return("test.go")
	mockChange.EXPECT().Patch().Return(mockPatch, nil)
	mockPatch.EXPECT().FilePatches().Return([]vcs.FilePatch{mockFilePatch})
	mockFilePatch.EXPECT().Chunks().Return([]vcs.Chunk{mockChunk})
	mockChunk.EXPECT().Type().Return(vcs.ChunkAdd)
	mockChunk.EXPECT().Content().Return("line1\nline2\n")

	ctx := context.Background()
	analyzer := New(WithOpener(mockOpener))
	result, err := analyzer.Analyze(ctx, "/fake/repo", nil)

	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}
	if result == nil {
		t.Fatal("Expected non-nil result")
	}
	if len(result.Files) != 1 {
		t.Errorf("Expected 1 file, got %d", len(result.Files))
	}
	if result.Files[0].RelativePath != "test.go" {
		t.Errorf("Expected test.go, got %s", result.Files[0].RelativePath)
	}
	if result.Files[0].LinesAdded != 2 {
		t.Errorf("Expected 2 lines added, got %d", result.Files[0].LinesAdded)
	}
}

func TestChurnAnalyzer_InitialCommitSkipped(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit := mocks.NewMockCommit(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	// Commit with no parents (initial commit)
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		return fn(mockCommit)
	})
	mockIter.EXPECT().Close().Return()

	mockCommit.EXPECT().NumParents().Return(0)

	ctx := context.Background()
	analyzer := New(WithOpener(mockOpener))
	result, err := analyzer.Analyze(ctx, "/fake/repo", nil)

	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}
	if len(result.Files) != 0 {
		t.Errorf("Expected 0 files for initial commit, got %d", len(result.Files))
	}
}

func TestChurnAnalyzer_DeletedFile(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit := mocks.NewMockCommit(t)
	mockParent := mocks.NewMockCommit(t)
	mockTree := mocks.NewMockTree(t)
	mockParentTree := mocks.NewMockTree(t)
	mockChange := mocks.NewMockChange(t)
	mockPatch := mocks.NewMockPatch(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		return fn(mockCommit)
	})
	mockIter.EXPECT().Close().Return()

	mockCommit.EXPECT().NumParents().Return(1)
	mockCommit.EXPECT().Parent(0).Return(mockParent, nil)
	mockCommit.EXPECT().Tree().Return(mockTree, nil)
	mockCommit.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  time.Now(),
	}).Maybe()

	mockParent.EXPECT().Tree().Return(mockParentTree, nil)
	mockParentTree.EXPECT().Diff(mockTree).Return(vcs.Changes{mockChange}, nil)

	// Deleted file - ToName is empty, FromName has the filename
	mockChange.EXPECT().ToName().Return("")
	mockChange.EXPECT().FromName().Return("deleted.go")
	mockChange.EXPECT().Patch().Return(mockPatch, nil)
	mockPatch.EXPECT().FilePatches().Return([]vcs.FilePatch{})

	ctx := context.Background()
	analyzer := New(WithOpener(mockOpener))
	result, err := analyzer.Analyze(ctx, "/fake/repo", nil)

	if err != nil {
		t.Fatalf("AnalyzeRepo() error = %v", err)
	}
	if len(result.Files) != 1 {
		t.Errorf("Expected 1 file, got %d", len(result.Files))
	}
	if result.Files[0].RelativePath != "deleted.go" {
		t.Errorf("Expected deleted.go, got %s", result.Files[0].RelativePath)
	}
}

func TestWithOpener(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	analyzer := New(WithOpener(mockOpener))

	if analyzer.opener != mockOpener {
		t.Error("Expected opener to be set to mock")
	}
}

// Verify hash type is correct
func TestChurnAnalyzer_HashType(t *testing.T) {
	// Verify plumbing.Hash is the correct type
	var _ plumbing.Hash
}
