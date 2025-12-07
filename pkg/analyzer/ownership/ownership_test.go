package ownership

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
)

func TestNew(t *testing.T) {
	a := New()
	defer a.Close()

	if !a.excludeTrivial {
		t.Error("Expected excludeTrivial to be true by default")
	}
}

func TestNewWithOptions(t *testing.T) {
	a := New(WithIncludeTrivial())
	defer a.Close()

	if a.excludeTrivial {
		t.Error("Expected excludeTrivial to be false")
	}
}

func TestAnalyzer_Analyze(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Create file with author Alice
	content := `package main

func foo() {
	println("hello")
	println("world")
}
`
	writeFileAndCommitWithAuthor(t, repo, repoPath, "main.go", content, "initial", "Alice")

	analyzer := New()
	defer analyzer.Close()

	files := []string{filepath.Join(repoPath, "main.go")}
	analysis, err := analyzer.Analyze(context.Background(), repoPath, files)
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(analysis.Files) != 1 {
		t.Fatalf("Expected 1 file, got %d", len(analysis.Files))
	}

	fo := analysis.Files[0]
	if fo.PrimaryOwner != "Alice" {
		t.Errorf("PrimaryOwner = %q, want %q", fo.PrimaryOwner, "Alice")
	}
	if !fo.IsSilo {
		t.Error("Expected file to be a silo (single contributor)")
	}
	if fo.OwnershipPercent != 100.0 {
		t.Errorf("OwnershipPercent = %f, want 100.0", fo.OwnershipPercent)
	}
}

func TestAnalyzer_MultipleContributors(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Alice writes initial code
	content1 := `package main

func foo() {
	println("alice1")
	println("alice2")
	println("alice3")
}
`
	writeFileAndCommitWithAuthor(t, repo, repoPath, "main.go", content1, "alice initial", "Alice")

	// Bob adds more code
	content2 := `package main

func foo() {
	println("alice1")
	println("alice2")
	println("alice3")
}

func bar() {
	println("bob1")
	println("bob2")
}
`
	writeFileAndCommitWithAuthor(t, repo, repoPath, "main.go", content2, "bob adds bar", "Bob")

	analyzer := New()
	defer analyzer.Close()

	files := []string{filepath.Join(repoPath, "main.go")}
	analysis, err := analyzer.Analyze(context.Background(), repoPath, files)
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(analysis.Files) != 1 {
		t.Fatalf("Expected 1 file, got %d", len(analysis.Files))
	}

	fo := analysis.Files[0]
	if fo.IsSilo {
		t.Error("Expected file to NOT be a silo (multiple contributors)")
	}
	if len(fo.Contributors) < 2 {
		t.Errorf("Expected at least 2 contributors, got %d", len(fo.Contributors))
	}
}

func TestAnalyzer_NoGitRepo(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := New()
	defer analyzer.Close()

	_, err := analyzer.Analyze(context.Background(), tmpDir, []string{})
	if err == nil {
		t.Error("Expected error for non-git directory")
	}
}

func TestAnalyzer_BusFactor(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// Alice owns most of the code
	for i := 0; i < 5; i++ {
		content := "package main\n// line " + string(rune('a'+i)) + "\n"
		writeFileAndCommitWithAuthor(t, repo, repoPath, "alice"+string(rune('0'+i))+".go", content, "alice file", "Alice")
	}

	// Bob owns one file
	writeFileAndCommitWithAuthor(t, repo, repoPath, "bob.go", "package main\n// bob\n", "bob file", "Bob")

	analyzer := New()
	defer analyzer.Close()

	files := []string{
		filepath.Join(repoPath, "alice0.go"),
		filepath.Join(repoPath, "alice1.go"),
		filepath.Join(repoPath, "alice2.go"),
		filepath.Join(repoPath, "alice3.go"),
		filepath.Join(repoPath, "alice4.go"),
		filepath.Join(repoPath, "bob.go"),
	}
	analysis, err := analyzer.Analyze(context.Background(), repoPath, files)
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Bus factor should be 1 (Alice alone covers > 50%)
	if analysis.Summary.BusFactor != 1 {
		t.Errorf("BusFactor = %d, want 1", analysis.Summary.BusFactor)
	}
}

func TestIsTrivialLine(t *testing.T) {
	tests := []struct {
		line string
		want bool
	}{
		{"", true},
		{"   ", true},
		{"{", true},
		{"}", true},
		{"import \"fmt\"", true},
		{"from module import something", true},
		{"#include <stdio.h>", true},
		{"package main", true},
		{"func foo() {", false},
		{"println(\"hello\")", false},
		{"x := 42", false},
	}

	for _, tt := range tests {
		t.Run(tt.line, func(t *testing.T) {
			got := isTrivialLine(tt.line)
			if got != tt.want {
				t.Errorf("isTrivialLine(%q) = %v, want %v", tt.line, got, tt.want)
			}
		})
	}
}

func TestAnalyzer_SortsByConcentration(t *testing.T) {
	tmpDir := t.TempDir()
	repoPath := filepath.Join(tmpDir, "repo")

	repo := initGitRepo(t, repoPath)

	// File with single owner (high concentration)
	writeFileAndCommitWithAuthor(t, repo, repoPath, "silo.go", "package main\nfunc a(){}\nfunc b(){}\n", "silo", "Alice")

	// File with multiple owners (lower concentration)
	content1 := "package main\nfunc x(){}\nfunc y(){}\n"
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", content1, "shared1", "Bob")
	content2 := "package main\nfunc x(){}\nfunc y(){}\nfunc z(){}\n"
	writeFileAndCommitWithAuthor(t, repo, repoPath, "shared.go", content2, "shared2", "Charlie")

	analyzer := New()
	defer analyzer.Close()

	files := []string{
		filepath.Join(repoPath, "silo.go"),
		filepath.Join(repoPath, "shared.go"),
	}
	analysis, err := analyzer.Analyze(context.Background(), repoPath, files)
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(analysis.Files) < 2 {
		t.Skip("Not enough files to test sorting")
	}

	// Should be sorted by concentration descending
	for i := 0; i < len(analysis.Files)-1; i++ {
		if analysis.Files[i].Concentration < analysis.Files[i+1].Concentration {
			t.Error("Expected files to be sorted by concentration descending")
		}
	}
}

func TestCalculateConcentration(t *testing.T) {
	tests := []struct {
		name         string
		contributors []Contributor
		want         float64
	}{
		{"empty", nil, 0},
		{"single contributor", []Contributor{{Percentage: 100}}, 1.0},
		{"two equal", []Contributor{{Percentage: 50}, {Percentage: 50}}, 0.5},
		{"dominant owner", []Contributor{{Percentage: 80}, {Percentage: 20}}, 0.8},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateConcentration(tt.contributors)
			if got != tt.want {
				t.Errorf("CalculateConcentration() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestAnalysis_CalculateSummary(t *testing.T) {
	tests := []struct {
		name           string
		files          []FileOwnership
		wantBusFactor  int
		wantSiloCount  int
		wantTotalFiles int
	}{
		{
			name:           "empty",
			files:          nil,
			wantBusFactor:  0,
			wantSiloCount:  0,
			wantTotalFiles: 0,
		},
		{
			name: "single silo",
			files: []FileOwnership{
				{
					IsSilo:        true,
					Concentration: 1.0,
					Contributors:  []Contributor{{Name: "Alice", LinesOwned: 100}},
				},
			},
			wantBusFactor:  1,
			wantSiloCount:  1,
			wantTotalFiles: 1,
		},
		{
			name: "multiple files",
			files: []FileOwnership{
				{
					IsSilo:        false,
					Concentration: 0.6,
					Contributors: []Contributor{
						{Name: "Alice", LinesOwned: 60},
						{Name: "Bob", LinesOwned: 40},
					},
				},
				{
					IsSilo:        true,
					Concentration: 1.0,
					Contributors:  []Contributor{{Name: "Charlie", LinesOwned: 50}},
				},
			},
			wantSiloCount:  1,
			wantTotalFiles: 2,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analysis := &Analysis{Files: tt.files}
			analysis.CalculateSummary()

			if analysis.Summary.TotalFiles != tt.wantTotalFiles {
				t.Errorf("TotalFiles = %d, want %d", analysis.Summary.TotalFiles, tt.wantTotalFiles)
			}
			if analysis.Summary.SiloCount != tt.wantSiloCount {
				t.Errorf("SiloCount = %d, want %d", analysis.Summary.SiloCount, tt.wantSiloCount)
			}
		})
	}
}

// Helper functions

func initGitRepo(t *testing.T, path string) *git.Repository {
	t.Helper()
	if err := os.MkdirAll(path, 0755); err != nil {
		t.Fatalf("Failed to create repo dir: %v", err)
	}
	repo, err := git.PlainInit(path, false)
	if err != nil {
		t.Fatalf("Failed to init git repo: %v", err)
	}
	return repo
}

func writeFileAndCommitWithAuthor(t *testing.T, repo *git.Repository, repoPath, filename, content, message, authorName string) {
	t.Helper()

	filePath := filepath.Join(repoPath, filename)
	if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write file %s: %v", filename, err)
	}

	w, err := repo.Worktree()
	if err != nil {
		t.Fatalf("Failed to get worktree: %v", err)
	}

	if _, err := w.Add(filename); err != nil {
		t.Fatalf("Failed to add file %s: %v", filename, err)
	}

	_, err = w.Commit(message, &git.CommitOptions{
		Author: &object.Signature{
			Name:  authorName,
			Email: authorName + "@example.com",
			When:  time.Now(),
		},
	})
	if err != nil {
		t.Fatalf("Failed to commit: %v", err)
	}
}
