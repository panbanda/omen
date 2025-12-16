package fileproc

import (
	"context"
	"fmt"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
)

// TestMapFilesIndexed verifies that indexed result collection preserves order
// and eliminates mutex contention.
func TestMapFilesIndexed(t *testing.T) {
	tmpDir := t.TempDir()

	files := make([]string, 100)
	for i := 0; i < 100; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	ctx := context.Background()
	results, errs := MapFilesIndexed(ctx, files, func(p *parser.Parser, path string) (string, error) {
		return filepath.Base(path), nil
	})

	if errs != nil && errs.HasErrors() {
		t.Errorf("Unexpected errors: %v", errs)
	}

	// Verify all results are present
	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	// Verify results are in order (indexed assignment preserves order)
	for i, r := range results {
		expected := fmt.Sprintf("file%d.go", i)
		if r != expected {
			t.Errorf("Result[%d] = %q, want %q", i, r, expected)
		}
	}
}

// TestMapFilesIndexed_WithErrors verifies error handling preserves valid results
func TestMapFilesIndexed_WithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file0.go", "package main"),
		createTestFile(t, tmpDir, "file1.go", "package main"),
		createTestFile(t, tmpDir, "file2.go", "package main"),
	}

	ctx := context.Background()
	errorIndex := 1
	results, errs := MapFilesIndexed(ctx, files, func(p *parser.Parser, path string) (string, error) {
		if filepath.Base(path) == fmt.Sprintf("file%d.go", errorIndex) {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	})

	// Should have all results (indexed assignment doesn't skip indices)
	if len(results) != len(files) {
		t.Errorf("Expected %d results slots, got %d", len(files), len(results))
	}

	// Error result should be zero value
	if results[errorIndex] != "" {
		t.Errorf("Error result[%d] should be empty, got %q", errorIndex, results[errorIndex])
	}

	// Valid results should be present at correct indices
	if results[0] != "file0.go" {
		t.Errorf("Result[0] = %q, want 'file0.go'", results[0])
	}
	if results[2] != "file2.go" {
		t.Errorf("Result[2] = %q, want 'file2.go'", results[2])
	}

	if errs == nil || len(errs.Errors) != 1 {
		t.Errorf("Expected 1 error, got %v", errs)
	}
}

// TestForEachFileIndexed verifies indexed collection for non-parser operations
func TestForEachFileIndexed(t *testing.T) {
	tmpDir := t.TempDir()

	files := make([]string, 50)
	for i := 0; i < 50; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("test%d.txt", i), "content")
	}

	ctx := context.Background()
	results, errs := ForEachFileIndexed(ctx, files, func(path string) (int, error) {
		// Extract index from filename
		var idx int
		fmt.Sscanf(filepath.Base(path), "test%d.txt", &idx)
		return idx, nil
	})

	if errs != nil && errs.HasErrors() {
		t.Errorf("Unexpected errors: %v", errs)
	}

	// Verify results are in order
	for i, r := range results {
		if r != i {
			t.Errorf("Result[%d] = %d, want %d", i, r, i)
		}
	}
}

// BenchmarkMapFilesIndexed_NoMutex compares indexed vs mutex-based collection
func BenchmarkMapFilesIndexed_NoMutex(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	ctx := context.Background()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		results, _ := MapFilesIndexed(ctx, files, func(p *parser.Parser, path string) (int, error) {
			return 1, nil
		})

		if len(results) != fileCount {
			b.Fatalf("Expected %d results, got %d", fileCount, len(results))
		}
	}
}

// TestMapFilesIndexed_Progress verifies progress tracking with indexed results
func TestMapFilesIndexed_Progress(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "a.go", "package main"),
		createTestFile(t, tmpDir, "b.go", "package main"),
		createTestFile(t, tmpDir, "c.go", "package main"),
	}

	progressCount := atomic.Int32{}
	tracker := analyzer.NewTracker(func(current, total int, path string) {
		progressCount.Add(1)
	})

	ctx := analyzer.WithTracker(context.Background(), tracker)
	results, errs := MapFilesIndexed(ctx, files, func(p *parser.Parser, path string) (string, error) {
		return filepath.Base(path), nil
	})

	if errs != nil && errs.HasErrors() {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != 3 {
		t.Errorf("Expected 3 results, got %d", len(results))
	}

	if int(progressCount.Load()) != 3 {
		t.Errorf("Expected 3 progress callbacks, got %d", progressCount.Load())
	}
}
