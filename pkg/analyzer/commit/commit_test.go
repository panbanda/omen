package commit

import (
	"testing"
	"time"

	"github.com/go-git/go-git/v6/plumbing"
	"github.com/go-git/go-git/v6/plumbing/object"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
)

func TestAnalyzeCommit(t *testing.T) {
	hash := plumbing.NewHash("abc123def456abc123def456abc123def456abc1")

	mockTree := mocks.NewMockTree(t)
	mockTree.EXPECT().Entries().Return([]vcs.TreeEntry{
		{Path: "main.go", Size: 100, IsDir: false},
		{Path: "util.go", Size: 50, IsDir: false},
	}, nil)
	mockTree.EXPECT().File("main.go").Return([]byte("package main\n\nfunc main() {\n}\n"), nil)
	mockTree.EXPECT().File("util.go").Return([]byte("package main\n\nfunc helper() {\n}\n"), nil)

	mockCommit := mocks.NewMockCommit(t)
	mockCommit.EXPECT().Tree().Return(mockTree, nil)
	mockCommit.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  time.Now(),
	})

	mockRepo := mocks.NewMockRepository(t)
	mockRepo.EXPECT().CommitObject(hash).Return(mockCommit, nil)

	a := New()
	defer a.Close()

	result, err := a.AnalyzeCommit(mockRepo, hash)
	require.NoError(t, err)

	assert.Equal(t, hash.String(), result.CommitHash)
	assert.Equal(t, 2, result.Complexity.Summary.TotalFiles)
}

func TestAnalyzeCommits(t *testing.T) {
	hash1 := plumbing.NewHash("abc123def456abc123def456abc123def456abc1")
	hash2 := plumbing.NewHash("abc123def456abc123def456abc123def456abc2")
	hash3 := plumbing.NewHash("abc123def456abc123def456abc123def456abc3")

	mockRepo := mocks.NewMockRepository(t)

	for i, hash := range []plumbing.Hash{hash1, hash2, hash3} {
		mockTree := mocks.NewMockTree(t)
		mockTree.EXPECT().Entries().Return([]vcs.TreeEntry{
			{Path: "main.go", Size: 100, IsDir: false},
		}, nil)
		mockTree.EXPECT().File("main.go").Return([]byte("package main\n\nfunc main() {\n}\n"), nil)

		mockCommit := mocks.NewMockCommit(t)
		mockCommit.EXPECT().Tree().Return(mockTree, nil)
		mockCommit.EXPECT().Author().Return(object.Signature{
			Name:  "Test Author",
			Email: "test@example.com",
			When:  time.Now().Add(-time.Duration(i) * time.Hour),
		})

		mockRepo.EXPECT().CommitObject(hash).Return(mockCommit, nil)
	}

	a := New()
	defer a.Close()

	results, err := a.AnalyzeCommits(mockRepo, []plumbing.Hash{hash1, hash2, hash3})
	require.NoError(t, err)

	assert.Len(t, results, 3)
	for _, r := range results {
		assert.NotEmpty(t, r.CommitHash)
		assert.NotNil(t, r.Complexity)
	}
}

func TestAnalyzeTrend(t *testing.T) {
	hash1 := plumbing.NewHash("abc123def456abc123def456abc123def456abc1")
	hash2 := plumbing.NewHash("abc123def456abc123def456abc123def456abc2")

	now := time.Now()

	// Create mock commits for the iterator
	mockIterCommit1 := mocks.NewMockCommit(t)
	mockIterCommit1.EXPECT().Hash().Return(hash1)
	mockIterCommit1.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  now.Add(-1 * time.Hour),
	})

	mockIterCommit2 := mocks.NewMockCommit(t)
	mockIterCommit2.EXPECT().Hash().Return(hash2)
	mockIterCommit2.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  now.Add(-2 * time.Hour),
	})

	// Create mock iterator
	mockIter := mocks.NewMockCommitIterator(t)
	commits := []vcs.Commit{mockIterCommit1, mockIterCommit2}
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		for _, c := range commits {
			if err := fn(c); err != nil {
				return err
			}
		}
		return nil
	})
	mockIter.EXPECT().Close().Return()

	// Create mock commits for AnalyzeCommit calls (separate from iterator)
	mockTree1 := mocks.NewMockTree(t)
	mockTree1.EXPECT().Entries().Return([]vcs.TreeEntry{
		{Path: "main.go", Size: 100, IsDir: false},
	}, nil)
	mockTree1.EXPECT().File("main.go").Return([]byte("package main\n\nfunc main() {\n}\n"), nil)

	mockAnalyzeCommit1 := mocks.NewMockCommit(t)
	mockAnalyzeCommit1.EXPECT().Tree().Return(mockTree1, nil)
	mockAnalyzeCommit1.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  now.Add(-1 * time.Hour),
	})

	mockTree2 := mocks.NewMockTree(t)
	mockTree2.EXPECT().Entries().Return([]vcs.TreeEntry{
		{Path: "main.go", Size: 100, IsDir: false},
	}, nil)
	mockTree2.EXPECT().File("main.go").Return([]byte("package main\n\nfunc main() {\n\tfmt.Println(\"hello\")\n}\n"), nil)

	mockAnalyzeCommit2 := mocks.NewMockCommit(t)
	mockAnalyzeCommit2.EXPECT().Tree().Return(mockTree2, nil)
	mockAnalyzeCommit2.EXPECT().Author().Return(object.Signature{
		Name:  "Test Author",
		Email: "test@example.com",
		When:  now.Add(-2 * time.Hour),
	})

	mockRepo := mocks.NewMockRepository(t)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)
	mockRepo.EXPECT().CommitObject(hash1).Return(mockAnalyzeCommit1, nil)
	mockRepo.EXPECT().CommitObject(hash2).Return(mockAnalyzeCommit2, nil)

	a := New()
	defer a.Close()

	trends, err := a.AnalyzeTrend(mockRepo, 7*24*time.Hour)
	require.NoError(t, err)

	assert.Len(t, trends.Commits, 2)
}
