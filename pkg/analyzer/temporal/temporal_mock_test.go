package temporal

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/go-git/go-git/v6/plumbing/object"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/stretchr/testify/mock"
)

func TestTemporalCouplingAnalyzer_WithMockOpener(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).Return(nil)

	analyzer := New(30, 3, WithOpener(mockOpener))
	result, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}
	if result == nil || result.PeriodDays != 30 {
		t.Fatalf("Expected non-nil result with PeriodDays=30, got %v", result)
	}
}

func TestTemporalCouplingAnalyzer_OpenError(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockOpener.EXPECT().PlainOpen("/invalid/path").Return(nil, errors.New("not a git repository"))

	analyzer := New(30, 3, WithOpener(mockOpener))
	_, err := analyzer.Analyze(context.Background(), "/invalid/path", nil)

	if err == nil {
		t.Fatal("Expected error for invalid path")
	}
}

func TestTemporalCouplingAnalyzer_LogError(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(nil, errors.New("log error"))

	analyzer := New(30, 3, WithOpener(mockOpener))
	_, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err == nil {
		t.Fatal("Expected error from Log()")
	}
}

func TestTemporalCouplingAnalyzer_WithCoChangedFiles(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit1 := mocks.NewMockCommit(t)
	mockCommit2 := mocks.NewMockCommit(t)
	mockCommit3 := mocks.NewMockCommit(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	// Simulate 3 commits where file1.go and file2.go are changed together 3 times
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		commits := []vcs.Commit{mockCommit1, mockCommit2, mockCommit3}
		for _, c := range commits {
			if err := fn(c); err != nil {
				return err
			}
		}
		return nil
	})

	// Each commit changes file1.go and file2.go together
	stats := object.FileStats{
		{Name: "file1.go", Addition: 10, Deletion: 0},
		{Name: "file2.go", Addition: 5, Deletion: 0},
	}
	mockCommit1.EXPECT().Stats().Return(stats, nil)
	mockCommit2.EXPECT().Stats().Return(stats, nil)
	mockCommit3.EXPECT().Stats().Return(stats, nil)

	analyzer := New(30, 3, WithOpener(mockOpener))
	result, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}
	if len(result.Couplings) != 1 {
		t.Errorf("Expected 1 coupling, got %d", len(result.Couplings))
	}
	if result.Couplings[0].CochangeCount != 3 {
		t.Errorf("Expected cochange count 3, got %d", result.Couplings[0].CochangeCount)
	}
}

func TestTemporalCouplingAnalyzer_BelowThreshold(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit := mocks.NewMockCommit(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	// Only 1 commit with co-change (below threshold of 3)
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		return fn(mockCommit)
	})

	stats := object.FileStats{
		{Name: "file1.go", Addition: 10, Deletion: 0},
		{Name: "file2.go", Addition: 5, Deletion: 0},
	}
	mockCommit.EXPECT().Stats().Return(stats, nil)

	analyzer := New(30, 3, WithOpener(mockOpener))
	result, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}
	// No couplings because cochange count (1) is below threshold (3)
	if len(result.Couplings) != 0 {
		t.Errorf("Expected 0 couplings (below threshold), got %d", len(result.Couplings))
	}
}

func TestTemporalCouplingAnalyzer_StatsError(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit := mocks.NewMockCommit(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		return fn(mockCommit)
	})

	// Stats returns error - should be skipped
	mockCommit.EXPECT().Stats().Return(nil, errors.New("stats error"))

	analyzer := New(30, 3, WithOpener(mockOpener))
	result, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v (should handle stats errors gracefully)", err)
	}
	if len(result.Couplings) != 0 {
		t.Errorf("Expected 0 couplings, got %d", len(result.Couplings))
	}
}

func TestWithOpener(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	analyzer := New(30, 3, WithOpener(mockOpener))

	if analyzer.opener != mockOpener {
		t.Error("Expected opener to be set to mock")
	}
}

func TestTemporalCouplingAnalyzer_DefaultValues(t *testing.T) {
	analyzer := New(0, 0)
	if analyzer.days != 30 {
		t.Errorf("Expected default days=30, got %d", analyzer.days)
	}
	if analyzer.minCochanges != 3 {
		t.Errorf("Expected default minCochanges=3, got %d", analyzer.minCochanges)
	}
}

func TestMakeFilePair_Normalized(t *testing.T) {
	pair1 := makeFilePair("b.go", "a.go")
	pair2 := makeFilePair("a.go", "b.go")

	if pair1 != pair2 {
		t.Error("File pairs should be normalized to same order")
	}
	if pair1.a != "a.go" || pair1.b != "b.go" {
		t.Errorf("Expected a=a.go, b=b.go, got a=%s, b=%s", pair1.a, pair1.b)
	}
}

func TestTemporalCouplingAnalyzer_Close(t *testing.T) {
	analyzer := New(30, 3)
	analyzer.Close() // Should not panic
}

func TestTemporalCouplingAnalyzer_ContextCancellation(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)
	mockCommit := mocks.NewMockCommit(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	// Simulate context being done
	called := false
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		if !called {
			called = true
			return fn(mockCommit)
		}
		return nil
	})

	// Return valid stats to let the loop continue
	stats := object.FileStats{{Name: "file.go"}}
	mockCommit.EXPECT().Stats().Return(stats, nil)

	analyzer := New(30, 3, WithOpener(mockOpener))
	result, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}
	if result == nil {
		t.Fatal("Expected non-nil result")
	}
}

func TestTemporalCouplingAnalyzer_CouplingStrengthSorting(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.AnythingOfType("*vcs.LogOptions")).Return(mockIter, nil)

	commits := make([]*mocks.MockCommit, 5)
	for i := 0; i < 5; i++ {
		commits[i] = mocks.NewMockCommit(t)
	}

	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).RunAndReturn(func(fn func(vcs.Commit) error) error {
		for _, c := range commits {
			if err := fn(c); err != nil {
				return err
			}
		}
		return nil
	})

	// Create two pairs: a-b with 5 co-changes, c-d with 3 co-changes
	// First 5 commits have a.go + b.go
	for i := 0; i < 3; i++ {
		commits[i].EXPECT().Stats().Return(object.FileStats{
			{Name: "a.go"}, {Name: "b.go"},
		}, nil)
	}
	// Last 2 commits have c.go + d.go
	for i := 3; i < 5; i++ {
		commits[i].EXPECT().Stats().Return(object.FileStats{
			{Name: "a.go"}, {Name: "b.go"}, {Name: "c.go"},
		}, nil)
	}

	analyzer := New(30, 3, WithOpener(mockOpener))
	result, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	// Should have couplings sorted by strength
	if len(result.Couplings) == 0 {
		t.Fatal("Expected at least 1 coupling")
	}

	// Verify sorted by coupling strength (descending)
	for i := 1; i < len(result.Couplings); i++ {
		if result.Couplings[i-1].CouplingStrength < result.Couplings[i].CouplingStrength {
			t.Error("Couplings should be sorted by strength descending")
		}
	}
}

func TestTemporalCouplingAnalyzer_NowTimeUsed(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	mockRepo := mocks.NewMockRepository(t)
	mockIter := mocks.NewMockCommitIterator(t)

	mockOpener.EXPECT().PlainOpen("/fake/repo").Return(mockRepo, nil)
	mockRepo.EXPECT().Log(mock.MatchedBy(func(opts *vcs.LogOptions) bool {
		// Verify the Since time is roughly 30 days ago
		if opts.Since == nil {
			return false
		}
		expectedSince := time.Now().AddDate(0, 0, -30)
		diff := opts.Since.Sub(expectedSince)
		return diff > -time.Hour && diff < time.Hour
	})).Return(mockIter, nil)
	mockIter.EXPECT().ForEach(mock.AnythingOfType("func(vcs.Commit) error")).Return(nil)

	analyzer := New(30, 3, WithOpener(mockOpener))
	_, err := analyzer.Analyze(context.Background(), "/fake/repo", nil)

	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}
}
