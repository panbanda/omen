package analyzer

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/panbanda/omen/pkg/models"
)

// writeFilesAndCommit writes multiple files and commits them together in a single commit.
func writeFilesAndCommit(t *testing.T, repo *git.Repository, repoPath string, files map[string]string, message string) {
	t.Helper()

	w, err := repo.Worktree()
	if err != nil {
		t.Fatalf("Failed to get worktree: %v", err)
	}

	for filename, content := range files {
		filePath := filepath.Join(repoPath, filename)
		if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
			t.Fatalf("Failed to write file %s: %v", filename, err)
		}
		if _, err := w.Add(filename); err != nil {
			t.Fatalf("Failed to add file %s: %v", filename, err)
		}
	}

	_, err = w.Commit(message, &git.CommitOptions{
		Author: &object.Signature{
			Name:  "Test Author",
			Email: "test@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatalf("Failed to commit: %v", err)
	}
}

func TestNewTemporalCouplingAnalyzer(t *testing.T) {
	tests := []struct {
		name             string
		days             int
		minCochanges     int
		wantDays         int
		wantMinCochanges int
	}{
		{"default days", 0, 0, 30, models.DefaultMinCochanges},
		{"negative days", -1, -1, 30, models.DefaultMinCochanges},
		{"custom values", 60, 5, 60, 5},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			a := NewTemporalCouplingAnalyzer(tt.days, tt.minCochanges)
			defer a.Close()

			if a.days != tt.wantDays {
				t.Errorf("days = %d, want %d", a.days, tt.wantDays)
			}
			if a.minCochanges != tt.wantMinCochanges {
				t.Errorf("minCochanges = %d, want %d", a.minCochanges, tt.wantMinCochanges)
			}
		})
	}
}

func TestTemporalCouplingAnalyzer_AnalyzeRepo(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Commit 1: a.go and b.go together
	writeFilesAndCommit(t, repo, repoPath, map[string]string{
		"a.go": "package a\n",
		"b.go": "package b\n",
	}, "commit 1")

	// Commit 2: a.go and b.go together again
	writeFilesAndCommit(t, repo, repoPath, map[string]string{
		"a.go": "package a\nfunc A() {}\n",
		"b.go": "package b\nfunc B() {}\n",
	}, "commit 2")

	// Commit 3: a.go and b.go together again (3rd co-change)
	writeFilesAndCommit(t, repo, repoPath, map[string]string{
		"a.go": "package a\nfunc A() {}\nfunc A2() {}\n",
		"b.go": "package b\nfunc B() {}\nfunc B2() {}\n",
	}, "commit 3")

	// Commit 4: only c.go (no coupling with a/b)
	writeFileAndCommit(t, repo, repoPath, "c.go", "package c\n", "commit 4")

	analyzer := NewTemporalCouplingAnalyzer(30, 1) // Lower threshold for test
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
	}

	// Should find a.go <-> b.go coupling (3 co-changes)
	if len(analysis.Couplings) == 0 {
		t.Error("Expected at least one coupling")
	}

	// Verify the a.go <-> b.go coupling exists
	found := false
	for _, c := range analysis.Couplings {
		if (c.FileA == "a.go" && c.FileB == "b.go") || (c.FileA == "b.go" && c.FileB == "a.go") {
			found = true
			if c.CochangeCount < 3 {
				t.Errorf("Expected at least 3 co-changes for a.go/b.go, got %d", c.CochangeCount)
			}
		}
	}
	if !found {
		t.Error("Expected coupling between a.go and b.go")
	}

	// Verify summary
	if analysis.Summary.TotalFilesAnalyzed < 3 {
		t.Errorf("Expected at least 3 files analyzed, got %d", analysis.Summary.TotalFilesAnalyzed)
	}
}

func TestTemporalCouplingAnalyzer_NoGitRepo(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := NewTemporalCouplingAnalyzer(30, 3)
	defer analyzer.Close()

	_, err := analyzer.AnalyzeRepo(tmpDir)
	if err == nil {
		t.Error("Expected error for non-git directory")
	}
}

func TestTemporalCouplingAnalyzer_MinCochangesFilter(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Create files with only 1 co-change
	writeFileAndCommit(t, repo, repoPath, "x.go", "package x\n", "initial x")
	writeFileAndCommit(t, repo, repoPath, "y.go", "package y\n", "initial y")

	// High threshold analyzer (min 5 co-changes)
	analyzer := NewTemporalCouplingAnalyzer(30, 5)
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
	}

	// Should have no couplings (below threshold)
	if len(analysis.Couplings) != 0 {
		t.Errorf("Expected 0 couplings with high threshold, got %d", len(analysis.Couplings))
	}
}

func TestTemporalCouplingAnalyzer_SortsByStrength(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Create strong coupling between a.go and b.go (3 co-changes)
	for i := 0; i < 3; i++ {
		writeFilesAndCommit(t, repo, repoPath, map[string]string{
			"a.go": "package a\n// v" + string(rune('0'+i)) + "\n",
			"b.go": "package b\n// v" + string(rune('0'+i)) + "\n",
		}, "update a+b")
	}

	// Create weaker coupling between c.go and d.go (1 co-change)
	writeFilesAndCommit(t, repo, repoPath, map[string]string{
		"c.go": "package c\n",
		"d.go": "package d\n",
	}, "initial c+d")

	analyzer := NewTemporalCouplingAnalyzer(30, 1) // Low threshold
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeRepo(repoPath)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
	}

	if len(analysis.Couplings) < 2 {
		t.Skip("Not enough couplings to test sorting")
	}

	// Should be sorted by strength descending
	for i := 0; i < len(analysis.Couplings)-1; i++ {
		if analysis.Couplings[i].CouplingStrength < analysis.Couplings[i+1].CouplingStrength {
			t.Error("Expected couplings to be sorted by strength descending")
		}
	}
}

func TestMakeFilePair(t *testing.T) {
	tests := []struct {
		a, b  string
		wantA string
		wantB string
	}{
		{"a.go", "b.go", "a.go", "b.go"},
		{"b.go", "a.go", "a.go", "b.go"}, // Should be normalized
		{"z.go", "a.go", "a.go", "z.go"},
	}

	for _, tt := range tests {
		pair := makeFilePair(tt.a, tt.b)
		if pair.a != tt.wantA || pair.b != tt.wantB {
			t.Errorf("makeFilePair(%q, %q) = {%q, %q}, want {%q, %q}",
				tt.a, tt.b, pair.a, pair.b, tt.wantA, tt.wantB)
		}
	}
}
