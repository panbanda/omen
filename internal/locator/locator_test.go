package locator

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer/repomap"
)

func TestLocate_ExactFilePath(t *testing.T) {
	// Create temp directory with a test file
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "service.go")
	if err := os.WriteFile(testFile, []byte("package main"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := Locate(testFile, nil)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Type != TargetFile {
		t.Errorf("expected type %q, got %q", TargetFile, result.Type)
	}
	if result.Path != testFile {
		t.Errorf("expected path %q, got %q", testFile, result.Path)
	}
}

func TestLocate_ExactFilePath_NotFound(t *testing.T) {
	result, err := Locate("/nonexistent/path/file.go", nil)

	// Should not be an error - just not found as exact path, will try other methods
	// But with no files list and no repo map, should return not found error
	if err == nil {
		t.Fatal("expected error for nonexistent path with no fallback")
	}
	if result != nil {
		t.Errorf("expected nil result, got %+v", result)
	}
}

func TestLocate_GlobPattern_SingleMatch(t *testing.T) {
	tmpDir := t.TempDir()
	subDir := filepath.Join(tmpDir, "services")
	if err := os.MkdirAll(subDir, 0755); err != nil {
		t.Fatal(err)
	}
	testFile := filepath.Join(subDir, "user.go")
	if err := os.WriteFile(testFile, []byte("package services"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := Locate("**/user.go", nil, WithBaseDir(tmpDir))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Type != TargetFile {
		t.Errorf("expected type %q, got %q", TargetFile, result.Type)
	}
	if result.Path != testFile {
		t.Errorf("expected path %q, got %q", testFile, result.Path)
	}
}

func TestLocate_GlobPattern_MultipleMatches(t *testing.T) {
	tmpDir := t.TempDir()
	dir1 := filepath.Join(tmpDir, "pkg", "a")
	dir2 := filepath.Join(tmpDir, "pkg", "b")
	if err := os.MkdirAll(dir1, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(dir2, 0755); err != nil {
		t.Fatal(err)
	}
	file1 := filepath.Join(dir1, "service.go")
	file2 := filepath.Join(dir2, "service.go")
	if err := os.WriteFile(file1, []byte("package a"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file2, []byte("package b"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := Locate("**/service.go", nil, WithBaseDir(tmpDir))
	if err != ErrAmbiguousMatch {
		t.Fatalf("expected ErrAmbiguousMatch, got %v", err)
	}

	if len(result.Candidates) != 2 {
		t.Fatalf("expected 2 candidates, got %d", len(result.Candidates))
	}
}

func TestLocate_Basename_SingleMatch(t *testing.T) {
	tmpDir := t.TempDir()
	subDir := filepath.Join(tmpDir, "services")
	if err := os.MkdirAll(subDir, 0755); err != nil {
		t.Fatal(err)
	}
	testFile := filepath.Join(subDir, "unique_file.go")
	if err := os.WriteFile(testFile, []byte("package services"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := Locate("unique_file.go", nil, WithBaseDir(tmpDir))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Type != TargetFile {
		t.Errorf("expected type %q, got %q", TargetFile, result.Type)
	}
	if result.Path != testFile {
		t.Errorf("expected path %q, got %q", testFile, result.Path)
	}
}

func TestLocate_Basename_MultipleMatches(t *testing.T) {
	tmpDir := t.TempDir()
	dir1 := filepath.Join(tmpDir, "pkg", "a")
	dir2 := filepath.Join(tmpDir, "pkg", "b")
	if err := os.MkdirAll(dir1, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(dir2, 0755); err != nil {
		t.Fatal(err)
	}
	file1 := filepath.Join(dir1, "handler.go")
	file2 := filepath.Join(dir2, "handler.go")
	if err := os.WriteFile(file1, []byte("package a"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file2, []byte("package b"), 0644); err != nil {
		t.Fatal(err)
	}

	result, err := Locate("handler.go", nil, WithBaseDir(tmpDir))
	if err != ErrAmbiguousMatch {
		t.Fatalf("expected ErrAmbiguousMatch, got %v", err)
	}

	if len(result.Candidates) != 2 {
		t.Fatalf("expected 2 candidates, got %d", len(result.Candidates))
	}
}

func TestLocate_Symbol_SingleMatch(t *testing.T) {
	repoMap := &repomap.Map{
		Symbols: []repomap.Symbol{
			{Name: "CreateUser", Kind: "function", File: "services/user.go", Line: 42},
			{Name: "DeleteUser", Kind: "function", File: "services/user.go", Line: 100},
		},
	}

	result, err := Locate("CreateUser", repoMap)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Type != TargetSymbol {
		t.Errorf("expected type %q, got %q", TargetSymbol, result.Type)
	}
	if result.Symbol == nil {
		t.Fatal("expected symbol, got nil")
	}
	if result.Symbol.Name != "CreateUser" {
		t.Errorf("expected name %q, got %q", "CreateUser", result.Symbol.Name)
	}
	if result.Symbol.File != "services/user.go" {
		t.Errorf("expected file %q, got %q", "services/user.go", result.Symbol.File)
	}
	if result.Symbol.Line != 42 {
		t.Errorf("expected line %d, got %d", 42, result.Symbol.Line)
	}
}

func TestLocate_Symbol_MultipleMatches(t *testing.T) {
	repoMap := &repomap.Map{
		Symbols: []repomap.Symbol{
			{Name: "CreateUser", Kind: "function", File: "services/user.go", Line: 42},
			{Name: "CreateUser", Kind: "method", File: "admin/admin.go", Line: 87},
		},
	}

	result, err := Locate("CreateUser", repoMap)
	if err != ErrAmbiguousMatch {
		t.Fatalf("expected ErrAmbiguousMatch, got %v", err)
	}

	if len(result.Candidates) != 2 {
		t.Fatalf("expected 2 candidates, got %d", len(result.Candidates))
	}
}

func TestLocate_Symbol_NotFound(t *testing.T) {
	repoMap := &repomap.Map{
		Symbols: []repomap.Symbol{
			{Name: "CreateUser", Kind: "function", File: "services/user.go", Line: 42},
		},
	}

	_, err := Locate("NonExistent", repoMap)
	if err != ErrNotFound {
		t.Fatalf("expected ErrNotFound, got %v", err)
	}
}
