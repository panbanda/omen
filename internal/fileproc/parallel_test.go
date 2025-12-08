package fileproc

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
)

func TestMapFiles(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.go", "package main\nfunc main() {}"),
		createTestFile(t, tmpDir, "file2.go", "package main\nfunc test() {}"),
		createTestFile(t, tmpDir, "file3.go", "package main\nfunc validate() {}"),
	}

	ctx := context.Background()
	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (string, error) {
		return filepath.Base(path), nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	resultMap := make(map[string]bool)
	for _, r := range results {
		resultMap[r] = true
	}

	expectedFiles := []string{"file1.go", "file2.go", "file3.go"}
	for _, expected := range expectedFiles {
		if !resultMap[expected] {
			t.Errorf("Missing expected result: %s", expected)
		}
	}
}

func TestMapFiles_EmptyFileList(t *testing.T) {
	ctx := context.Background()
	results, errs := MapFiles(ctx, []string{}, func(p *parser.Parser, path string) (string, error) {
		return path, nil
	})

	if results != nil {
		t.Errorf("Expected nil for empty file list, got %v", results)
	}
	if errs != nil {
		t.Errorf("Expected nil errors for empty file list, got %v", errs)
	}
}

func TestMapFiles_SingleFile(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "single.go", "package main")

	ctx := context.Background()
	results, errs := MapFiles(ctx, []string{file}, func(p *parser.Parser, path string) (int, error) {
		return 42, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != 1 {
		t.Fatalf("Expected 1 result, got %d", len(results))
	}

	if results[0] != 42 {
		t.Errorf("Expected result 42, got %d", results[0])
	}
}

func TestMapFiles_WithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good1.go", "package main"),
		createTestFile(t, tmpDir, "bad.go", "package main"),
		createTestFile(t, tmpDir, "good2.go", "package main"),
	}

	ctx := context.Background()
	processedCount := atomic.Int32{}
	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (string, error) {
		processedCount.Add(1)
		if filepath.Base(path) == "bad.go" {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	})

	if int(processedCount.Load()) != 3 {
		t.Errorf("Expected all 3 files to be processed, got %d", processedCount.Load())
	}

	if len(results) != 2 {
		t.Errorf("Expected 2 successful results (errors skipped), got %d", len(results))
	}

	if errs == nil {
		t.Error("Expected errors to be returned")
	} else if len(errs.Errors) != 1 {
		t.Errorf("Expected 1 error, got %d", len(errs.Errors))
	}
}

func TestMapFiles_ParserAvailable(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.go", "package main\nfunc main() {}")

	ctx := context.Background()
	results, errs := MapFiles(ctx, []string{file}, func(p *parser.Parser, path string) (bool, error) {
		if p == nil {
			t.Error("Parser should not be nil")
			return false, nil
		}

		result, err := p.ParseFile(path)
		if err != nil {
			return false, err
		}

		return result != nil && result.Tree != nil, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != 1 {
		t.Fatalf("Expected 1 result, got %d", len(results))
	}

	if !results[0] {
		t.Error("Parser should have successfully parsed the file")
	}
}

func TestMapFiles_WithProgress(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.go", "package main"),
		createTestFile(t, tmpDir, "file2.go", "package main"),
		createTestFile(t, tmpDir, "file3.go", "package main"),
		createTestFile(t, tmpDir, "file4.go", "package main"),
		createTestFile(t, tmpDir, "file5.go", "package main"),
	}

	progressCount := atomic.Int32{}
	tracker := analyzer.NewTracker(func(current, total int, path string) {
		progressCount.Add(1)
	})

	ctx := analyzer.WithTracker(context.Background(), tracker)
	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	if int(progressCount.Load()) != len(files) {
		t.Errorf("Expected progress callback %d times, got %d", len(files), progressCount.Load())
	}
}

func TestMapFiles_WithProgressNilTracker(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.go", "package main")

	// No tracker in context - should still work
	ctx := context.Background()
	results, errs := MapFiles(ctx, []string{file}, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}
	if len(results) != 1 {
		t.Errorf("Expected 1 result, got %d", len(results))
	}
}

func TestMapFiles_Cancellation(t *testing.T) {
	tmpDir := t.TempDir()

	// Create many files to increase chance of catching cancellation
	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	ctx, cancel := context.WithCancel(context.Background())

	var processed atomic.Int32
	go func() {
		// Cancel after some files have started processing
		for processed.Load() < 10 {
			runtime.Gosched()
		}
		cancel()
	}()

	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (string, error) {
		processed.Add(1)
		// Small delay to allow cancellation to take effect
		for i := 0; i < 1000; i++ {
			runtime.Gosched()
		}
		return filepath.Base(path), nil
	})

	// We should have partial results
	t.Logf("Processed %d files, got %d results", processed.Load(), len(results))

	// Should have some context canceled errors
	if errs != nil {
		hasContextError := false
		for _, e := range errs.Errors {
			if e.Err == context.Canceled {
				hasContextError = true
				break
			}
		}
		if !hasContextError {
			t.Log("No context.Canceled errors found (cancellation may have happened after all processing)")
		}
	}

	// Total results + errors should be <= fileCount
	errorCount := 0
	if errs != nil {
		errorCount = len(errs.Errors)
	}
	if len(results)+errorCount > fileCount {
		t.Errorf("Results (%d) + errors (%d) should not exceed file count (%d)",
			len(results), errorCount, fileCount)
	}
}

func TestMapFilesWithSizeLimit(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files of different sizes
	smallFile := createTestFile(t, tmpDir, "small.go", "package main")
	largeContent := make([]byte, 1024)
	for i := range largeContent {
		largeContent[i] = 'a'
	}
	largeFile := filepath.Join(tmpDir, "large.go")
	if err := os.WriteFile(largeFile, append([]byte("package main\n"), largeContent...), 0644); err != nil {
		t.Fatalf("failed to create large file: %v", err)
	}

	t.Run("with size limit", func(t *testing.T) {
		ctx := context.Background()
		results, errs := MapFilesWithSizeLimit(ctx, []string{smallFile, largeFile}, 100, func(p *parser.Parser, path string) (string, error) {
			return filepath.Base(path), nil
		})

		if len(results) != 1 {
			t.Errorf("Expected 1 result (small file only), got %d", len(results))
		}
		if errs == nil || len(errs.Errors) != 1 {
			t.Errorf("Expected 1 error for large file, got %v", errs)
		}
	})

	t.Run("no size limit", func(t *testing.T) {
		ctx := context.Background()
		results, errs := MapFilesWithSizeLimit(ctx, []string{smallFile, largeFile}, 0, func(p *parser.Parser, path string) (string, error) {
			return filepath.Base(path), nil
		})

		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 2 {
			t.Errorf("Expected 2 results with no limit, got %d", len(results))
		}
	})

	t.Run("stat error handling", func(t *testing.T) {
		ctx := context.Background()
		nonExistent := filepath.Join(tmpDir, "nonexistent.go")
		results, errs := MapFilesWithSizeLimit(ctx, []string{nonExistent}, 100, func(p *parser.Parser, path string) (string, error) {
			return filepath.Base(path), nil
		})

		if len(results) != 0 {
			t.Errorf("Expected 0 results, got %d", len(results))
		}
		if errs == nil || len(errs.Errors) != 1 {
			t.Errorf("Expected 1 error, got %v", errs)
		}
	})
}

func TestForEachFile(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.txt", "content1"),
		createTestFile(t, tmpDir, "file2.txt", "content2"),
		createTestFile(t, tmpDir, "file3.txt", "content3"),
	}

	ctx := context.Background()
	results, errs := ForEachFile(ctx, files, func(path string) (string, error) {
		return filepath.Base(path), nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	resultMap := make(map[string]bool)
	for _, r := range results {
		resultMap[r] = true
	}

	expectedFiles := []string{"file1.txt", "file2.txt", "file3.txt"}
	for _, expected := range expectedFiles {
		if !resultMap[expected] {
			t.Errorf("Missing expected result: %s", expected)
		}
	}
}

func TestForEachFile_EmptyFileList(t *testing.T) {
	ctx := context.Background()
	results, errs := ForEachFile(ctx, []string{}, func(path string) (int, error) {
		return 1, nil
	})

	if results != nil {
		t.Errorf("Expected nil for empty file list, got %v", results)
	}
	if errs != nil {
		t.Errorf("Expected nil errors for empty file list, got %v", errs)
	}
}

func TestForEachFile_WithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good1.txt", "content"),
		createTestFile(t, tmpDir, "bad.txt", "content"),
		createTestFile(t, tmpDir, "good2.txt", "content"),
	}

	ctx := context.Background()
	results, errs := ForEachFile(ctx, files, func(path string) (string, error) {
		if filepath.Base(path) == "bad.txt" {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	})

	if len(results) != 2 {
		t.Errorf("Expected 2 successful results, got %d", len(results))
	}

	if errs == nil || len(errs.Errors) != 1 {
		t.Errorf("Expected 1 error, got %v", errs)
	}
}

func TestForEachFile_WithProgress(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.txt", "content"),
		createTestFile(t, tmpDir, "file2.txt", "content"),
		createTestFile(t, tmpDir, "file3.txt", "content"),
		createTestFile(t, tmpDir, "file4.txt", "content"),
	}

	progressCount := atomic.Int32{}
	tracker := analyzer.NewTracker(func(current, total int, path string) {
		progressCount.Add(1)
	})

	ctx := analyzer.WithTracker(context.Background(), tracker)
	results, errs := ForEachFile(ctx, files, func(path string) (int, error) {
		return 1, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	if int(progressCount.Load()) != len(files) {
		t.Errorf("Expected progress callback %d times, got %d", len(files), progressCount.Load())
	}
}

func TestForEachFile_ProgressOnError(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good.txt", "content"),
		createTestFile(t, tmpDir, "bad.txt", "content"),
	}

	progressCount := atomic.Int32{}
	tracker := analyzer.NewTracker(func(current, total int, path string) {
		progressCount.Add(1)
	})

	ctx := analyzer.WithTracker(context.Background(), tracker)
	results, _ := ForEachFile(ctx, files, func(path string) (int, error) {
		if filepath.Base(path) == "bad.txt" {
			return 0, fmt.Errorf("error")
		}
		return 1, nil
	})

	if len(results) != 1 {
		t.Errorf("Expected 1 successful result, got %d", len(results))
	}

	if int(progressCount.Load()) != 2 {
		t.Errorf("Progress should be called even on errors, expected 2, got %d", progressCount.Load())
	}
}

func TestForEachFileWithResource(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.txt", "content1"),
		createTestFile(t, tmpDir, "file2.txt", "content2"),
		createTestFile(t, tmpDir, "file3.txt", "content3"),
	}

	t.Run("successful resource usage", func(t *testing.T) {
		type mockResource struct {
			id int
		}

		var resourceCount int32
		initResource := func() (*mockResource, error) {
			id := atomic.AddInt32(&resourceCount, 1)
			return &mockResource{id: int(id)}, nil
		}
		closeResource := func(r *mockResource) {
			// Resource cleanup
		}

		ctx := context.Background()
		results, errs := ForEachFileWithResource(ctx, files, initResource, closeResource, func(r *mockResource, path string) (string, error) {
			return fmt.Sprintf("%s:%d", filepath.Base(path), r.id), nil
		})

		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 3 {
			t.Errorf("Expected 3 results, got %d", len(results))
		}
	})

	t.Run("empty file list", func(t *testing.T) {
		ctx := context.Background()
		results, errs := ForEachFileWithResource(ctx, []string{}, func() (int, error) {
			return 0, nil
		}, func(r int) {}, func(r int, path string) (string, error) {
			return path, nil
		})

		if results != nil {
			t.Errorf("Expected nil for empty file list, got %v", results)
		}
		if errs != nil {
			t.Errorf("Expected nil errors for empty file list, got %v", errs)
		}
	})

	t.Run("resource init error", func(t *testing.T) {
		initResource := func() (int, error) {
			return 0, fmt.Errorf("init failed")
		}

		ctx := context.Background()
		results, errs := ForEachFileWithResource(ctx, files, initResource, func(r int) {}, func(r int, path string) (string, error) {
			return filepath.Base(path), nil
		})

		// With invalid resources, all files are skipped
		if len(results) != 0 {
			t.Errorf("Expected 0 results with failed init, got %d", len(results))
		}
		if errs == nil {
			t.Error("Expected errors for failed resource init")
		}
	})

	t.Run("processing error handling", func(t *testing.T) {
		initResource := func() (int, error) {
			return 42, nil
		}

		ctx := context.Background()
		results, errs := ForEachFileWithResource(ctx, files, initResource, func(r int) {}, func(r int, path string) (string, error) {
			if filepath.Base(path) == "file2.txt" {
				return "", fmt.Errorf("processing error")
			}
			return filepath.Base(path), nil
		})

		if len(results) != 2 {
			t.Errorf("Expected 2 results, got %d", len(results))
		}
		if errs == nil || len(errs.Errors) != 1 {
			t.Errorf("Expected 1 error, got %v", errs)
		}
	})

	t.Run("with nil closeResource", func(t *testing.T) {
		initResource := func() (int, error) {
			return 1, nil
		}

		ctx := context.Background()
		results, errs := ForEachFileWithResource(ctx, files, initResource, nil, func(r int, path string) (string, error) {
			return filepath.Base(path), nil
		})

		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 3 {
			t.Errorf("Expected 3 results, got %d", len(results))
		}
	})

	t.Run("with progress tracking", func(t *testing.T) {
		initResource := func() (int, error) {
			return 1, nil
		}

		progressCount := atomic.Int32{}
		tracker := analyzer.NewTracker(func(current, total int, path string) {
			progressCount.Add(1)
		})

		ctx := analyzer.WithTracker(context.Background(), tracker)
		results, errs := ForEachFileWithResource(ctx, files, initResource, nil, func(r int, path string) (string, error) {
			return filepath.Base(path), nil
		})

		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 3 {
			t.Errorf("Expected 3 results, got %d", len(results))
		}
		if int(progressCount.Load()) != 3 {
			t.Errorf("Expected progress callback 3 times, got %d", progressCount.Load())
		}
	})
}

func TestProcessingError(t *testing.T) {
	err := ProcessingError{Path: "/path/to/file.go", Err: fmt.Errorf("parse failed")}
	expected := "/path/to/file.go: parse failed"
	if err.Error() != expected {
		t.Errorf("Error() = %q, want %q", err.Error(), expected)
	}
}

func TestProcessingErrors(t *testing.T) {
	errs := &ProcessingErrors{}

	// Empty errors
	if errs.HasErrors() {
		t.Error("Empty ProcessingErrors should not have errors")
	}
	if errs.Error() != "no errors" {
		t.Errorf("Empty error message = %q, want 'no errors'", errs.Error())
	}

	// Single error
	errs.Add("/file1.go", fmt.Errorf("error1"))
	if !errs.HasErrors() {
		t.Error("ProcessingErrors with one error should have errors")
	}
	if errs.Error() != "/file1.go: error1" {
		t.Errorf("Single error message = %q", errs.Error())
	}

	// Multiple errors
	errs.Add("/file2.go", fmt.Errorf("error2"))
	if len(errs.Errors) != 2 {
		t.Errorf("Expected 2 errors, got %d", len(errs.Errors))
	}
	errMsg := errs.Error()
	if errMsg != "2 files failed to process (first: /file1.go: error1)" {
		t.Errorf("Multiple error message = %q", errMsg)
	}
}

func TestProcessingErrors_ThreadSafe(t *testing.T) {
	errs := &ProcessingErrors{}
	var wg sync.WaitGroup

	// Add errors concurrently
	for i := 0; i < 100; i++ {
		wg.Add(1)
		go func(n int) {
			defer wg.Done()
			errs.Add(fmt.Sprintf("/file%d.go", n), fmt.Errorf("error %d", n))
		}(i)
	}
	wg.Wait()

	if len(errs.Errors) != 100 {
		t.Errorf("Expected 100 errors, got %d", len(errs.Errors))
	}
}

func TestProcessingErrors_Unwrap(t *testing.T) {
	errs := &ProcessingErrors{}
	if errs.Unwrap() != nil {
		t.Error("Unwrap() should return nil")
	}

	errs.Add("/file.go", fmt.Errorf("error"))
	if errs.Unwrap() != nil {
		t.Error("Unwrap() should still return nil even with errors")
	}
}

func TestMapFiles_LargeFileSet(t *testing.T) {
	if testing.Short() {
		t.Skip("Skipping large file set test in short mode")
	}

	tmpDir := t.TempDir()

	fileCount := 1000
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	ctx := context.Background()
	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != fileCount {
		t.Errorf("Expected %d results, got %d", fileCount, len(results))
	}

	sum := 0
	for _, r := range results {
		sum += r
	}

	if sum != fileCount {
		t.Errorf("Expected sum of %d, got %d", fileCount, sum)
	}
}

func TestMapFiles_ActualParsing(t *testing.T) {
	tmpDir := t.TempDir()

	fileCount := 20
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		content := fmt.Sprintf("package main\nfunc test%d() {}", i)
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), content)
	}

	ctx := context.Background()
	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (string, error) {
		result, err := p.ParseFile(path)
		if err != nil {
			return "", err
		}
		if result == nil || result.Tree == nil {
			return "", fmt.Errorf("parse result or tree is nil")
		}
		return result.Path, nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}

	if len(results) != fileCount {
		t.Errorf("Expected %d results, got %d", fileCount, len(results))
	}

	resultSet := make(map[string]bool)
	for _, r := range results {
		resultSet[r] = true
	}

	for _, file := range files {
		if !resultSet[file] {
			t.Errorf("Missing result for file: %s", file)
		}
	}
}

func TestMapFiles_ReturnTypes(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.go", "package main")
	ctx := context.Background()

	t.Run("string result", func(t *testing.T) {
		results, errs := MapFiles(ctx, []string{file}, func(p *parser.Parser, path string) (string, error) {
			return "test", nil
		})
		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 1 || results[0] != "test" {
			t.Errorf("Expected ['test'], got %v", results)
		}
	})

	t.Run("int result", func(t *testing.T) {
		results, errs := MapFiles(ctx, []string{file}, func(p *parser.Parser, path string) (int, error) {
			return 42, nil
		})
		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 1 || results[0] != 42 {
			t.Errorf("Expected [42], got %v", results)
		}
	})

	t.Run("struct result", func(t *testing.T) {
		type Result struct {
			Path string
			OK   bool
		}
		results, errs := MapFiles(ctx, []string{file}, func(p *parser.Parser, path string) (Result, error) {
			return Result{Path: path, OK: true}, nil
		})
		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 1 || !results[0].OK {
			t.Errorf("Expected struct with OK=true, got %v", results)
		}
	})
}

func TestForEachFile_ReturnTypes(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.txt", "content")
	ctx := context.Background()

	t.Run("bool result", func(t *testing.T) {
		results, errs := ForEachFile(ctx, []string{file}, func(path string) (bool, error) {
			return true, nil
		})
		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 1 || !results[0] {
			t.Errorf("Expected [true], got %v", results)
		}
	})

	t.Run("slice result", func(t *testing.T) {
		results, errs := ForEachFile(ctx, []string{file}, func(path string) ([]string, error) {
			return []string{"a", "b"}, nil
		})
		if errs != nil {
			t.Errorf("Unexpected errors: %v", errs)
		}
		if len(results) != 1 || len(results[0]) != 2 {
			t.Errorf("Expected [['a', 'b']], got %v", results)
		}
	})
}

func TestTrackerIntegration(t *testing.T) {
	tmpDir := t.TempDir()
	files := []string{
		createTestFile(t, tmpDir, "file1.go", "package main"),
		createTestFile(t, tmpDir, "file2.go", "package main"),
		createTestFile(t, tmpDir, "file3.go", "package main"),
	}

	var currentValues []int
	var totalValues []int
	var paths []string
	var mu sync.Mutex

	tracker := analyzer.NewTracker(func(current, total int, path string) {
		mu.Lock()
		currentValues = append(currentValues, current)
		totalValues = append(totalValues, total)
		paths = append(paths, filepath.Base(path))
		mu.Unlock()
	})

	ctx := analyzer.WithTracker(context.Background(), tracker)
	results, errs := MapFiles(ctx, files, func(p *parser.Parser, path string) (string, error) {
		return filepath.Base(path), nil
	})

	if errs != nil {
		t.Errorf("Unexpected errors: %v", errs)
	}
	if len(results) != 3 {
		t.Errorf("Expected 3 results, got %d", len(results))
	}

	// Check tracker was called 3 times
	if len(paths) != 3 {
		t.Errorf("Expected 3 progress callbacks, got %d", len(paths))
	}

	// Check total was always 3
	for i, total := range totalValues {
		if total != 3 {
			t.Errorf("Progress callback %d: expected total=3, got %d", i, total)
		}
	}
}

func BenchmarkMapFiles_Parallel(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.go", i), "package main\nfunc test() {}")
	}

	ctx := context.Background()
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
}

func BenchmarkForEachFile(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.txt", i), "test content")
	}

	ctx := context.Background()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		results, _ := ForEachFile(ctx, files, func(path string) (int, error) {
			_, err := os.ReadFile(path)
			if err != nil {
				return 0, err
			}
			return 1, nil
		})

		if len(results) != fileCount {
			b.Fatalf("Expected %d results, got %d", fileCount, len(results))
		}
	}
}

func BenchmarkMapFiles_WithProgress(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	progressCount := atomic.Int32{}
	tracker := analyzer.NewTracker(func(current, total int, path string) {
		progressCount.Add(1)
	})

	ctx := analyzer.WithTracker(context.Background(), tracker)
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		progressCount.Store(0)
		results, _ := MapFiles(ctx, files, func(p *parser.Parser, path string) (int, error) {
			return 1, nil
		})

		if len(results) != fileCount {
			b.Fatalf("Expected %d results, got %d", fileCount, len(results))
		}
	}
}

func createTestFile(t testing.TB, dir, name, content string) string {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("failed to create test file %s: %v", name, err)
	}
	return path
}
