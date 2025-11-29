package analyzer

import (
	"path/filepath"
	"testing"
)

func TestNewOwnershipAnalyzer(t *testing.T) {
	a := NewOwnershipAnalyzer()
	defer a.Close()

	if !a.excludeTrivial {
		t.Error("Expected excludeTrivial to be true by default")
	}
}

func TestNewOwnershipAnalyzerWithOptions(t *testing.T) {
	a := NewOwnershipAnalyzer(WithOwnershipIncludeTrivial())
	defer a.Close()

	if a.excludeTrivial {
		t.Error("Expected excludeTrivial to be false")
	}
}

func TestOwnershipAnalyzer_AnalyzeRepo(t *testing.T) {
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

	analyzer := NewOwnershipAnalyzer()
	defer analyzer.Close()

	files := []string{filepath.Join(repoPath, "main.go")}
	analysis, err := analyzer.AnalyzeRepo(repoPath, files)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
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

func TestOwnershipAnalyzer_MultipleContributors(t *testing.T) {
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

	analyzer := NewOwnershipAnalyzer()
	defer analyzer.Close()

	files := []string{filepath.Join(repoPath, "main.go")}
	analysis, err := analyzer.AnalyzeRepo(repoPath, files)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
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

func TestOwnershipAnalyzer_NoGitRepo(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := NewOwnershipAnalyzer()
	defer analyzer.Close()

	_, err := analyzer.AnalyzeRepo(tmpDir, []string{})
	if err == nil {
		t.Error("Expected error for non-git directory")
	}
}

func TestOwnershipAnalyzer_BusFactor(t *testing.T) {
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

	analyzer := NewOwnershipAnalyzer()
	defer analyzer.Close()

	files := []string{
		filepath.Join(repoPath, "alice0.go"),
		filepath.Join(repoPath, "alice1.go"),
		filepath.Join(repoPath, "alice2.go"),
		filepath.Join(repoPath, "alice3.go"),
		filepath.Join(repoPath, "alice4.go"),
		filepath.Join(repoPath, "bob.go"),
	}
	analysis, err := analyzer.AnalyzeRepo(repoPath, files)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
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

func TestOwnershipAnalyzer_SortsByConcentration(t *testing.T) {
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

	analyzer := NewOwnershipAnalyzer()
	defer analyzer.Close()

	files := []string{
		filepath.Join(repoPath, "silo.go"),
		filepath.Join(repoPath, "shared.go"),
	}
	analysis, err := analyzer.AnalyzeRepo(repoPath, files)
	if err != nil {
		t.Fatalf("AnalyzeRepo failed: %v", err)
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
