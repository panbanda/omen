package scanner

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/panbanda/omen/pkg/config"
)

func TestNew(t *testing.T) {
	svc := New()
	if svc == nil {
		t.Fatal("New() returned nil")
	}
	if svc.config == nil {
		t.Error("config should not be nil")
	}
	if svc.opener == nil {
		t.Error("opener should not be nil")
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
