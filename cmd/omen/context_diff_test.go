package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// TestGetChangedFiles verifies git diff parsing
func TestGetChangedFiles(t *testing.T) {
	// Create a temp git repo for testing
	tmpDir := t.TempDir()

	// Initialize git repo
	cmds := [][]string{
		{"git", "init"},
		{"git", "config", "user.email", "test@test.com"},
		{"git", "config", "user.name", "Test"},
	}
	for _, args := range cmds {
		cmd := exec.Command(args[0], args[1:]...)
		cmd.Dir = tmpDir
		if err := cmd.Run(); err != nil {
			t.Fatalf("failed to run %v: %v", args, err)
		}
	}

	// Create initial file and commit
	file1 := filepath.Join(tmpDir, "file1.go")
	if err := os.WriteFile(file1, []byte("package main\n"), 0644); err != nil {
		t.Fatalf("failed to write file: %v", err)
	}

	cmd := exec.Command("git", "add", ".")
	cmd.Dir = tmpDir
	if err := cmd.Run(); err != nil {
		t.Fatalf("failed to git add: %v", err)
	}

	cmd = exec.Command("git", "commit", "-m", "initial")
	cmd.Dir = tmpDir
	if err := cmd.Run(); err != nil {
		t.Fatalf("failed to git commit: %v", err)
	}

	// Modify file
	if err := os.WriteFile(file1, []byte("package main\n\nfunc foo() {}\n"), 0644); err != nil {
		t.Fatalf("failed to modify file: %v", err)
	}

	// Get changed files (should include file1.go)
	files, err := getChangedFiles(tmpDir, "HEAD")
	if err != nil {
		t.Fatalf("getChangedFiles failed: %v", err)
	}

	if len(files) == 0 {
		t.Error("expected at least one changed file")
	}

	found := false
	for _, f := range files {
		if strings.Contains(f, "file1.go") {
			found = true
			break
		}
	}
	if !found {
		t.Errorf("expected file1.go in changed files, got: %v", files)
	}
}

// TestIsSupportedSourceFile tests the source file detection
func TestIsSupportedSourceFile(t *testing.T) {
	tests := []struct {
		path     string
		expected bool
	}{
		{"main.go", true},
		{"app.ts", true},
		{"index.js", true},
		{"lib.rs", true},
		{"script.py", true},
		{"README.md", false},
		{"config.json", false},
		{"Makefile", false},
		{".gitignore", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := isSupportedSourceFile(tt.path)
			if got != tt.expected {
				t.Errorf("isSupportedSourceFile(%q) = %v, want %v", tt.path, got, tt.expected)
			}
		})
	}
}

// TestGetGitDiffStat tests git diff stat retrieval
func TestGetGitDiffStat(t *testing.T) {
	// Create a temp git repo for testing
	tmpDir := t.TempDir()

	// Initialize git repo
	cmds := [][]string{
		{"git", "init"},
		{"git", "config", "user.email", "test@test.com"},
		{"git", "config", "user.name", "Test"},
	}
	for _, args := range cmds {
		cmd := exec.Command(args[0], args[1:]...)
		cmd.Dir = tmpDir
		if err := cmd.Run(); err != nil {
			t.Fatalf("failed to run %v: %v", args, err)
		}
	}

	// Create initial file and commit
	file1 := filepath.Join(tmpDir, "file1.go")
	if err := os.WriteFile(file1, []byte("package main\n"), 0644); err != nil {
		t.Fatalf("failed to write file: %v", err)
	}

	cmd := exec.Command("git", "add", ".")
	cmd.Dir = tmpDir
	if err := cmd.Run(); err != nil {
		t.Fatalf("failed to git add: %v", err)
	}

	cmd = exec.Command("git", "commit", "-m", "initial")
	cmd.Dir = tmpDir
	if err := cmd.Run(); err != nil {
		t.Fatalf("failed to git commit: %v", err)
	}

	// Modify file
	if err := os.WriteFile(file1, []byte("package main\n\nfunc foo() {}\nfunc bar() {}\n"), 0644); err != nil {
		t.Fatalf("failed to modify file: %v", err)
	}

	// Get diff stat
	stat, err := getGitDiffStat(tmpDir, "HEAD")
	if err != nil {
		t.Fatalf("getGitDiffStat failed: %v", err)
	}

	// Should contain file1.go and some stats
	if !strings.Contains(stat, "file1.go") {
		t.Errorf("expected file1.go in diff stat, got: %s", stat)
	}
}
