package analysis

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer/repomap"
)

func TestFocusedContext_File(t *testing.T) {
	// Create a temp directory with a test file
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "service.go")
	content := `package main

func CreateUser() error {
	// TODO: implement validation
	return nil
}
`
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:   testFile,
		BaseDir: tmpDir,
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Target.Type != "file" {
		t.Errorf("expected type 'file', got %q", result.Target.Type)
	}
	if result.Target.Path != testFile {
		t.Errorf("expected path %q, got %q", testFile, result.Target.Path)
	}

	// Should have complexity data
	if result.Complexity == nil {
		t.Error("expected complexity data")
	}
}

func TestFocusedContext_Symbol(t *testing.T) {
	// Create a temp directory with a test file
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "service.go")
	content := `package main

func CreateUser() error {
	return nil
}

func DeleteUser() error {
	return nil
}
`
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// Create a repo map with symbols
	rm := &repomap.Map{
		Symbols: []repomap.Symbol{
			{Name: "CreateUser", Kind: "function", File: testFile, Line: 3},
			{Name: "DeleteUser", Kind: "function", File: testFile, Line: 7},
		},
	}

	svc := New()
	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:   "CreateUser",
		BaseDir: tmpDir,
		RepoMap: rm,
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Target.Type != "symbol" {
		t.Errorf("expected type 'symbol', got %q", result.Target.Type)
	}
	if result.Target.Symbol == nil {
		t.Fatal("expected symbol in target")
	}
	if result.Target.Symbol.Name != "CreateUser" {
		t.Errorf("expected symbol name 'CreateUser', got %q", result.Target.Symbol.Name)
	}
}

func TestFocusedContext_NotFound(t *testing.T) {
	svc := New()
	_, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:   "nonexistent.go",
		BaseDir: t.TempDir(),
	})
	if err == nil {
		t.Fatal("expected error for nonexistent target")
	}
}

func TestFocusedContext_AmbiguousMatch(t *testing.T) {
	tmpDir := t.TempDir()
	dir1 := filepath.Join(tmpDir, "pkg", "a")
	dir2 := filepath.Join(tmpDir, "pkg", "b")
	if err := os.MkdirAll(dir1, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(dir2, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir1, "service.go"), []byte("package a"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir2, "service.go"), []byte("package b"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:   "service.go",
		BaseDir: tmpDir,
	})

	// Should return an error with candidates
	if err == nil {
		t.Fatal("expected error for ambiguous match")
	}
	if result == nil {
		t.Fatal("expected result with candidates")
	}
	if len(result.Candidates) != 2 {
		t.Errorf("expected 2 candidates, got %d", len(result.Candidates))
	}
}
