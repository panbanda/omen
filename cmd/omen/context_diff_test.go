package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// TestContextDiffFlag verifies the --diff flag is recognized
func TestContextDiffFlag(t *testing.T) {
	cmd := exec.Command("./omen", "context", "--help")
	cmd.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
	output, err := cmd.CombinedOutput()
	if err != nil {
		// Build omen first if not found
		build := exec.Command("go", "build", "-o", "omen", "./cmd/omen")
		build.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
		if buildErr := build.Run(); buildErr != nil {
			t.Fatalf("failed to build omen: %v", buildErr)
		}
		output, err = cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("failed to run omen context --help: %v, output: %s", err, output)
		}
	}

	if !strings.Contains(string(output), "--diff") {
		t.Error("--diff flag not found in help output")
	}
}

// TestContextDiffBase verifies the --base flag for diff comparison
func TestContextDiffBase(t *testing.T) {
	cmd := exec.Command("./omen", "context", "--help")
	cmd.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
	output, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("failed to run omen context --help: %v", err)
	}

	if !strings.Contains(string(output), "--base") {
		t.Error("--base flag not found in help output")
	}
}

// TestContextDiffOutput verifies diff context produces expected sections
func TestContextDiffOutput(t *testing.T) {
	if testing.Short() {
		t.Skip("skipping integration test in short mode")
	}

	// This test runs in the omen repo which has git history
	cmd := exec.Command("./omen", "context", ".", "--diff", "--base", "HEAD~1")
	cmd.Dir = filepath.Join(os.Getenv("PWD"), "..", "..")
	output, err := cmd.CombinedOutput()
	if err != nil {
		// May fail if no changes, that's ok
		if strings.Contains(string(output), "no changes") {
			t.Skip("no changes between HEAD and HEAD~1")
		}
		t.Fatalf("failed to run diff context: %v, output: %s", err, output)
	}

	outputStr := string(output)

	// Should have diff context header
	if !strings.Contains(outputStr, "# Diff Context") && !strings.Contains(outputStr, "# Change Context") {
		t.Error("expected diff context header in output")
	}

	// Should list changed files
	if !strings.Contains(outputStr, "Changed Files") && !strings.Contains(outputStr, "Modified") {
		t.Error("expected changed files section in output")
	}
}

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
	cmd.Run()

	cmd = exec.Command("git", "commit", "-m", "initial")
	cmd.Dir = tmpDir
	cmd.Run()

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

// Note: getChangedFiles is defined in context.go
