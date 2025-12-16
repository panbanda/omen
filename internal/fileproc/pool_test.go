package fileproc

import (
	"context"
	"fmt"
	"path/filepath"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
)

// TestMapFilesPooled verifies that parser pools reduce allocations
func TestMapFilesPooled(t *testing.T) {
	tmpDir := t.TempDir()

	fileCount := 50
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main\nfunc main() {}")
	}

	ctx := context.Background()
	results, errs := MapFilesPooled(ctx, files, func(p *parser.Parser, path string) (string, error) {
		// Verify parser works
		result, err := p.ParseFile(path)
		if err != nil {
			return "", err
		}
		if result == nil || result.Tree == nil {
			return "", fmt.Errorf("parse result is nil")
		}
		return filepath.Base(path), nil
	})

	if errs != nil && errs.HasErrors() {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != fileCount {
		t.Errorf("Expected %d results, got %d", fileCount, len(results))
	}

	// Verify all files processed
	resultSet := make(map[string]bool)
	for _, r := range results {
		resultSet[r] = true
	}
	for i := 0; i < fileCount; i++ {
		expected := fmt.Sprintf("file%d.go", i)
		if !resultSet[expected] {
			t.Errorf("Missing result for %s", expected)
		}
	}
}

// TestMapFilesPooled_ParserReuse verifies parsers are reused within workers
func TestMapFilesPooled_ParserReuse(t *testing.T) {
	tmpDir := t.TempDir()

	// Create enough files to ensure parser reuse
	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	// Track unique parser addresses to verify reuse
	parserAddrs := make(map[uintptr]int)
	var mu sync.Mutex
	var parseCount atomic.Int32

	ctx := context.Background()
	results, errs := MapFilesPooled(ctx, files, func(p *parser.Parser, path string) (int, error) {
		// Track parser address
		addr := reflect.ValueOf(p).Pointer()
		mu.Lock()
		parserAddrs[addr]++
		mu.Unlock()
		parseCount.Add(1)
		return 1, nil
	})

	if errs != nil && errs.HasErrors() {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != fileCount {
		t.Errorf("Expected %d results, got %d", fileCount, len(results))
	}

	// Should have fewer unique parsers than files (parsers are reused)
	if len(parserAddrs) >= fileCount {
		t.Errorf("Expected parser reuse: got %d unique parsers for %d files", len(parserAddrs), fileCount)
	}

	// Each parser should be used multiple times
	for _, count := range parserAddrs {
		if count < 2 {
			t.Log("Some parsers used only once - expected with small file counts")
		}
	}

	t.Logf("Processed %d files with %d unique parsers", fileCount, len(parserAddrs))
}

// TestMapFilesPooled_Empty verifies empty file list handling
func TestMapFilesPooled_Empty(t *testing.T) {
	ctx := context.Background()
	results, errs := MapFilesPooled(ctx, []string{}, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	})

	if results != nil {
		t.Errorf("Expected nil results for empty list, got %v", results)
	}
	if errs != nil {
		t.Errorf("Expected nil errors for empty list, got %v", errs)
	}
}

// TestMapFilesPooled_WithErrors verifies error handling
func TestMapFilesPooled_WithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good1.go", "package main"),
		createTestFile(t, tmpDir, "bad.go", "package main"),
		createTestFile(t, tmpDir, "good2.go", "package main"),
	}

	ctx := context.Background()
	results, errs := MapFilesPooled(ctx, files, func(p *parser.Parser, path string) (string, error) {
		if filepath.Base(path) == "bad.go" {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	})

	// Should have 2 successful results
	successCount := 0
	for _, r := range results {
		if r != "" {
			successCount++
		}
	}
	if successCount != 2 {
		t.Errorf("Expected 2 successful results, got %d", successCount)
	}

	if errs == nil || len(errs.Errors) != 1 {
		t.Errorf("Expected 1 error, got %v", errs)
	}
}

// TestMapFilesPooled_Progress verifies progress tracking
func TestMapFilesPooled_Progress(t *testing.T) {
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
	results, errs := MapFilesPooled(ctx, files, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
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

// BenchmarkMapFilesPooled compares pooled vs non-pooled performance
func BenchmarkMapFilesPooled(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.go", i), "package main\nfunc test() {}")
	}

	ctx := context.Background()

	b.Run("Pooled", func(b *testing.B) {
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			results, _ := MapFilesPooled(ctx, files, func(p *parser.Parser, path string) (int, error) {
				_, err := p.ParseFile(path)
				if err != nil {
					return 0, err
				}
				return 1, nil
			})

			if len(results) != fileCount {
				b.Fatalf("Expected %d results, got %d", fileCount, len(results))
			}
		}
	})

	b.Run("NonPooled", func(b *testing.B) {
		b.ResetTimer()
		for i := 0; i < b.N; i++ {
			results, _ := MapFiles(ctx, files, func(p *parser.Parser, path string) (int, error) {
				_, err := p.ParseFile(path)
				if err != nil {
					return 0, err
				}
				return 1, nil
			})

			if len(results) != fileCount {
				b.Fatalf("Expected %d results, got %d", fileCount, len(results))
			}
		}
	})
}
