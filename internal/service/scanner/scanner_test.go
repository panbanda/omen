package scanner

import (
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/panbanda/omen/pkg/config"
)

func TestNew(t *testing.T) {
	svc := New()
	if svc == nil || svc.config == nil || svc.opener == nil {
		t.Fatal("New() returned nil or has nil config/opener")
	}
}

func TestNewWithConfig(t *testing.T) {
	cfg := &config.Config{}
	svc := New(WithConfig(cfg))
	if svc.config != cfg {
		t.Error("WithConfig did not set config")
	}
}

func TestNewWithOpener(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	svc := New(WithOpener(mockOpener))
	if svc.opener != mockOpener {
		t.Error("WithOpener did not set opener")
	}
}

func TestScanPaths_Empty(t *testing.T) {
	svc := New()
	result, err := svc.ScanPaths(nil)
	if err != nil {
		t.Fatalf("ScanPaths() error = %v", err)
	}
	if result == nil {
		t.Fatal("result should not be nil")
	}
	// Scanning current directory, may or may not have files
}

func TestScanPaths_InvalidPath(t *testing.T) {
	svc := New()
	_, err := svc.ScanPaths([]string{"/nonexistent/path/that/does/not/exist"})
	if err == nil {
		t.Error("expected error for nonexistent path")
	}
}

func TestScanPaths_ValidDir(t *testing.T) {
	// Create temp directory with Go file
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\n"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.ScanPaths([]string{tmpDir})
	if err != nil {
		t.Fatalf("ScanPaths() error = %v", err)
	}
	if len(result.Files) != 1 {
		t.Errorf("expected 1 file, got %d", len(result.Files))
	}
	if result.Files[0] != goFile {
		t.Errorf("expected %s, got %s", goFile, result.Files[0])
	}
}

func TestScanPaths_LanguageGroups(t *testing.T) {
	tmpDir := t.TempDir()

	// Create Go and Python files
	goFile := filepath.Join(tmpDir, "test.go")
	pyFile := filepath.Join(tmpDir, "test.py")

	if err := os.WriteFile(goFile, []byte("package main\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(pyFile, []byte("print('hello')\n"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.ScanPaths([]string{tmpDir})
	if err != nil {
		t.Fatalf("ScanPaths() error = %v", err)
	}

	if len(result.Files) != 2 {
		t.Errorf("expected 2 files, got %d", len(result.Files))
	}
	if len(result.LanguageGroups) == 0 {
		t.Error("expected language groups to be populated")
	}
}

func TestScanPathsForGit_NotGitRepo(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\n"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()

	// Not required - should succeed even without git
	result, err := svc.ScanPathsForGit([]string{tmpDir}, false)
	if err != nil {
		t.Fatalf("ScanPathsForGit() error = %v", err)
	}
	if result.RepoRoot != "" {
		t.Error("expected empty repo root for non-git directory")
	}
}

func TestScanPathsForGit_GitRequired(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\n"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()

	// Required - should fail without git
	_, err := svc.ScanPathsForGit([]string{tmpDir}, true)
	if err == nil {
		t.Error("expected error when git is required but not present")
	}

	var gitErr *GitError
	if _, ok := err.(*GitError); !ok {
		t.Errorf("expected *GitError, got %T", gitErr)
	}
}

func TestFilterBySize(t *testing.T) {
	tmpDir := t.TempDir()

	// Create small file
	smallFile := filepath.Join(tmpDir, "small.txt")
	if err := os.WriteFile(smallFile, []byte("small"), 0644); err != nil {
		t.Fatal(err)
	}

	// Create larger file
	largeFile := filepath.Join(tmpDir, "large.txt")
	largeContent := make([]byte, 1000)
	if err := os.WriteFile(largeFile, largeContent, 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	filtered, skipped := svc.FilterBySize([]string{smallFile, largeFile}, 100)

	if len(filtered) != 1 {
		t.Errorf("expected 1 filtered file, got %d", len(filtered))
	}
	if skipped != 1 {
		t.Errorf("expected 1 skipped, got %d", skipped)
	}
}

func TestFilterBySize_NoLimit(t *testing.T) {
	files := []string{"a.go", "b.go"}
	svc := New()
	filtered, skipped := svc.FilterBySize(files, 0)

	if len(filtered) != 2 {
		t.Errorf("expected 2 files, got %d", len(filtered))
	}
	if skipped != 0 {
		t.Errorf("expected 0 skipped, got %d", skipped)
	}
}

func TestPathError(t *testing.T) {
	err := &PathError{Path: "/foo", Err: os.ErrNotExist}
	expected := "invalid path /foo: file does not exist"
	if err.Error() != expected {
		t.Errorf("got %q, want %q", err.Error(), expected)
	}
	if err.Unwrap() != os.ErrNotExist {
		t.Error("Unwrap returned wrong error")
	}
}

func TestScanError(t *testing.T) {
	err := &ScanError{Path: "/foo", Err: os.ErrPermission}
	expected := "failed to scan directory /foo: permission denied"
	if err.Error() != expected {
		t.Errorf("got %q, want %q", err.Error(), expected)
	}
	if err.Unwrap() != os.ErrPermission {
		t.Error("Unwrap returned wrong error")
	}
}

func TestGitError(t *testing.T) {
	err := &GitError{Err: os.ErrNotExist}
	if err.Error() == "" {
		t.Error("expected non-empty error message")
	}
	if err.Unwrap() != os.ErrNotExist {
		t.Error("Unwrap returned wrong error")
	}
}

// createTestGitRepo creates a temporary git repository with test files
func createTestGitRepo(t *testing.T) string {
	t.Helper()
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create and commit a test file
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	return tmpDir
}

func runGit(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git %v failed: %v\n%s", args, err, out)
	}
}

func TestScanPathsForGit_InGitRepo(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()

	// Should find git root
	result, err := svc.ScanPathsForGit([]string{repoPath}, true)
	if err != nil {
		t.Fatalf("ScanPathsForGit() error = %v", err)
	}
	if result.RepoRoot == "" {
		t.Error("expected non-empty repo root for git directory")
	}
	if len(result.Files) == 0 {
		t.Error("expected at least one file")
	}
}

func TestScanPathsForGit_EmptyPaths(t *testing.T) {
	// Use current directory which is likely a git repo
	svc := New()

	result, err := svc.ScanPathsForGit(nil, false)
	if err != nil {
		// May fail if not in git repo - that's ok
		t.Skip("not in git repo, skipping")
	}
	if result == nil {
		t.Fatal("result should not be nil")
	}
}

func TestScanPaths_MultiplePaths(t *testing.T) {
	tmpDir1 := t.TempDir()
	tmpDir2 := t.TempDir()

	goFile1 := filepath.Join(tmpDir1, "test1.go")
	goFile2 := filepath.Join(tmpDir2, "test2.go")

	if err := os.WriteFile(goFile1, []byte("package main\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(goFile2, []byte("package main\n"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.ScanPaths([]string{tmpDir1, tmpDir2})
	if err != nil {
		t.Fatalf("ScanPaths() error = %v", err)
	}
	if len(result.Files) != 2 {
		t.Errorf("expected 2 files, got %d", len(result.Files))
	}
}
