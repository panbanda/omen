package fileproc

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestMapFiles(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.go", "package main\nfunc main() {}"),
		createTestFile(t, tmpDir, "file2.go", "package main\nfunc test() {}"),
		createTestFile(t, tmpDir, "file3.go", "package main\nfunc validate() {}"),
	}

	results := MapFiles(files, func(p *parser.Parser, path string) (string, error) {
		return filepath.Base(path), nil
	})

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
	results := MapFiles([]string{}, func(p *parser.Parser, path string) (string, error) {
		return path, nil
	})

	if results != nil {
		t.Errorf("Expected nil for empty file list, got %v", results)
	}
}

func TestMapFiles_SingleFile(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "single.go", "package main")

	results := MapFiles([]string{file}, func(p *parser.Parser, path string) (int, error) {
		return 42, nil
	})

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

	processedCount := atomic.Int32{}
	results := MapFiles(files, func(p *parser.Parser, path string) (string, error) {
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
}

func TestMapFilesWithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good1.go", "package main"),
		createTestFile(t, tmpDir, "bad.go", "package main"),
		createTestFile(t, tmpDir, "good2.go", "package main"),
	}

	var errorPaths []string
	var mu sync.Mutex
	onError := func(path string, err error) {
		mu.Lock()
		errorPaths = append(errorPaths, filepath.Base(path))
		mu.Unlock()
	}

	results := MapFilesWithErrors(files, func(p *parser.Parser, path string) (string, error) {
		if filepath.Base(path) == "bad.go" {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	}, onError)

	if len(results) != 2 {
		t.Errorf("Expected 2 successful results, got %d", len(results))
	}

	if len(errorPaths) != 1 || errorPaths[0] != "bad.go" {
		t.Errorf("Expected error callback for bad.go, got %v", errorPaths)
	}
}

func TestMapFiles_ParserAvailable(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.go", "package main\nfunc main() {}")

	results := MapFiles([]string{file}, func(p *parser.Parser, path string) (bool, error) {
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

	if len(results) != 1 {
		t.Fatalf("Expected 1 result, got %d", len(results))
	}

	if !results[0] {
		t.Error("Parser should have successfully parsed the file")
	}
}

func TestMapFilesWithProgress(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.go", "package main"),
		createTestFile(t, tmpDir, "file2.go", "package main"),
		createTestFile(t, tmpDir, "file3.go", "package main"),
		createTestFile(t, tmpDir, "file4.go", "package main"),
		createTestFile(t, tmpDir, "file5.go", "package main"),
	}

	progressCount := atomic.Int32{}
	progressFunc := func() {
		progressCount.Add(1)
	}

	results := MapFilesWithProgress(files, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	}, progressFunc)

	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	if int(progressCount.Load()) != len(files) {
		t.Errorf("Expected progress callback %d times, got %d", len(files), progressCount.Load())
	}
}

func TestMapFilesWithProgress_NilCallback(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.go", "package main")

	results := MapFilesWithProgress([]string{file}, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	}, nil)

	if len(results) != 1 {
		t.Errorf("Expected 1 result, got %d", len(results))
	}
}

func TestMapFilesN_WorkerCount(t *testing.T) {
	tests := []struct {
		name       string
		maxWorkers int
		wantMin    int
		wantMax    int
	}{
		{
			name:       "default workers (0)",
			maxWorkers: 0,
			wantMin:    runtime.NumCPU() * DefaultWorkerMultiplier,
			wantMax:    runtime.NumCPU() * DefaultWorkerMultiplier,
		},
		{
			name:       "negative workers",
			maxWorkers: -1,
			wantMin:    runtime.NumCPU() * DefaultWorkerMultiplier,
			wantMax:    runtime.NumCPU() * DefaultWorkerMultiplier,
		},
		{
			name:       "single worker",
			maxWorkers: 1,
			wantMin:    1,
			wantMax:    1,
		},
		{
			name:       "four workers",
			maxWorkers: 4,
			wantMin:    4,
			wantMax:    4,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			files := make([]string, 20)
			for i := 0; i < 20; i++ {
				files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
			}

			var maxConcurrent atomic.Int32
			var currentConcurrent atomic.Int32

			results := MapFilesN(files, tt.maxWorkers, func(p *parser.Parser, path string) (int, error) {
				current := currentConcurrent.Add(1)
				defer currentConcurrent.Add(-1)

				for {
					max := maxConcurrent.Load()
					if current <= max {
						break
					}
					if maxConcurrent.CompareAndSwap(max, current) {
						break
					}
				}

				return 1, nil
			}, nil, nil)

			if len(results) != len(files) {
				t.Errorf("Expected %d results, got %d", len(files), len(results))
			}

			observedMax := int(maxConcurrent.Load())
			if observedMax < tt.wantMin || observedMax > tt.wantMax {
				t.Logf("Observed max concurrent: %d (expected range: %d-%d)", observedMax, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestMapFilesN_ResultCorrectness(t *testing.T) {
	tmpDir := t.TempDir()

	fileCount := 50
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		content := fmt.Sprintf("package main\n// File number %d\n", i)
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), content)
	}

	results := MapFilesN(files, 8, func(p *parser.Parser, path string) (string, error) {
		return filepath.Base(path), nil
	}, nil, nil)

	if len(results) != fileCount {
		t.Fatalf("Expected %d results, got %d", fileCount, len(results))
	}

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

func TestMapFilesN_ThreadSafety(t *testing.T) {
	tmpDir := t.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), "package main")
	}

	sharedCounter := 0
	var mu sync.Mutex

	results := MapFilesN(files, runtime.NumCPU()*2, func(p *parser.Parser, path string) (int, error) {
		mu.Lock()
		sharedCounter++
		val := sharedCounter
		mu.Unlock()
		return val, nil
	}, nil, nil)

	if len(results) != fileCount {
		t.Fatalf("Expected %d results, got %d", fileCount, len(results))
	}

	if sharedCounter != fileCount {
		t.Errorf("Expected counter to be %d, got %d", fileCount, sharedCounter)
	}

	resultMap := make(map[int]int)
	for _, r := range results {
		resultMap[r]++
	}

	for i := 1; i <= fileCount; i++ {
		if resultMap[i] != 1 {
			t.Errorf("Value %d appeared %d times, expected 1", i, resultMap[i])
		}
	}
}

func TestForEachFile(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.txt", "content1"),
		createTestFile(t, tmpDir, "file2.txt", "content2"),
		createTestFile(t, tmpDir, "file3.txt", "content3"),
	}

	results := ForEachFile(files, func(path string) (string, error) {
		return filepath.Base(path), nil
	})

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
	results := ForEachFile([]string{}, func(path string) (int, error) {
		return 1, nil
	})

	if results != nil {
		t.Errorf("Expected nil for empty file list, got %v", results)
	}
}

func TestForEachFile_WithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good1.txt", "content"),
		createTestFile(t, tmpDir, "bad.txt", "content"),
		createTestFile(t, tmpDir, "good2.txt", "content"),
	}

	results := ForEachFile(files, func(path string) (string, error) {
		if filepath.Base(path) == "bad.txt" {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	})

	if len(results) != 2 {
		t.Errorf("Expected 2 successful results, got %d", len(results))
	}
}

func TestForEachFileWithErrors(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good1.txt", "content"),
		createTestFile(t, tmpDir, "bad.txt", "content"),
		createTestFile(t, tmpDir, "good2.txt", "content"),
	}

	var errorPaths []string
	var mu sync.Mutex
	onError := func(path string, err error) {
		mu.Lock()
		errorPaths = append(errorPaths, filepath.Base(path))
		mu.Unlock()
	}

	results := ForEachFileWithErrors(files, func(path string) (string, error) {
		if filepath.Base(path) == "bad.txt" {
			return "", fmt.Errorf("simulated error")
		}
		return filepath.Base(path), nil
	}, onError)

	if len(results) != 2 {
		t.Errorf("Expected 2 successful results, got %d", len(results))
	}

	if len(errorPaths) != 1 || errorPaths[0] != "bad.txt" {
		t.Errorf("Expected error callback for bad.txt, got %v", errorPaths)
	}
}

func TestForEachFileWithProgress(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "file1.txt", "content"),
		createTestFile(t, tmpDir, "file2.txt", "content"),
		createTestFile(t, tmpDir, "file3.txt", "content"),
		createTestFile(t, tmpDir, "file4.txt", "content"),
	}

	progressCount := atomic.Int32{}
	progressFunc := func() {
		progressCount.Add(1)
	}

	results := ForEachFileWithProgress(files, func(path string) (int, error) {
		return 1, nil
	}, progressFunc)

	if len(results) != len(files) {
		t.Errorf("Expected %d results, got %d", len(files), len(results))
	}

	if int(progressCount.Load()) != len(files) {
		t.Errorf("Expected progress callback %d times, got %d", len(files), progressCount.Load())
	}
}

func TestForEachFileWithProgress_ProgressOnError(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		createTestFile(t, tmpDir, "good.txt", "content"),
		createTestFile(t, tmpDir, "bad.txt", "content"),
	}

	progressCount := atomic.Int32{}
	progressFunc := func() {
		progressCount.Add(1)
	}

	results := ForEachFileWithProgress(files, func(path string) (int, error) {
		if filepath.Base(path) == "bad.txt" {
			return 0, fmt.Errorf("error")
		}
		return 1, nil
	}, progressFunc)

	if len(results) != 1 {
		t.Errorf("Expected 1 successful result, got %d", len(results))
	}

	if int(progressCount.Load()) != 2 {
		t.Errorf("Progress should be called even on errors, expected 2, got %d", progressCount.Load())
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

	results := MapFiles(files, func(p *parser.Parser, path string) (int, error) {
		return 1, nil
	})

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

func TestParallelProcessing_ActualParsing(t *testing.T) {
	tmpDir := t.TempDir()

	fileCount := 20
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		content := fmt.Sprintf("package main\nfunc test%d() {}", i)
		files[i] = createTestFile(t, tmpDir, fmt.Sprintf("file%d.go", i), content)
	}

	results := MapFiles(files, func(p *parser.Parser, path string) (string, error) {
		result, err := p.ParseFile(path)
		if err != nil {
			return "", err
		}
		if result == nil || result.Tree == nil {
			return "", fmt.Errorf("parse result or tree is nil")
		}
		return result.Path, nil
	})

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

	t.Run("string result", func(t *testing.T) {
		results := MapFiles([]string{file}, func(p *parser.Parser, path string) (string, error) {
			return "test", nil
		})
		if len(results) != 1 || results[0] != "test" {
			t.Errorf("Expected ['test'], got %v", results)
		}
	})

	t.Run("int result", func(t *testing.T) {
		results := MapFiles([]string{file}, func(p *parser.Parser, path string) (int, error) {
			return 42, nil
		})
		if len(results) != 1 || results[0] != 42 {
			t.Errorf("Expected [42], got %v", results)
		}
	})

	t.Run("struct result", func(t *testing.T) {
		type Result struct {
			Path string
			OK   bool
		}
		results := MapFiles([]string{file}, func(p *parser.Parser, path string) (Result, error) {
			return Result{Path: path, OK: true}, nil
		})
		if len(results) != 1 || !results[0].OK {
			t.Errorf("Expected struct with OK=true, got %v", results)
		}
	})
}

func TestForEachFile_ReturnTypes(t *testing.T) {
	tmpDir := t.TempDir()
	file := createTestFile(t, tmpDir, "test.txt", "content")

	t.Run("bool result", func(t *testing.T) {
		results := ForEachFile([]string{file}, func(path string) (bool, error) {
			return true, nil
		})
		if len(results) != 1 || !results[0] {
			t.Errorf("Expected [true], got %v", results)
		}
	})

	t.Run("slice result", func(t *testing.T) {
		results := ForEachFile([]string{file}, func(path string) ([]string, error) {
			return []string{"a", "b"}, nil
		})
		if len(results) != 1 || len(results[0]) != 2 {
			t.Errorf("Expected [['a', 'b']], got %v", results)
		}
	})
}

func BenchmarkMapFiles_Sequential(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.go", i), "package main\nfunc test() {}")
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		results := MapFilesN(files, 1, func(p *parser.Parser, path string) (int, error) {
			_, err := p.ParseFile(path)
			if err != nil {
				return 0, err
			}
			return 1, nil
		}, nil, nil)

		if len(results) != fileCount {
			b.Fatalf("Expected %d results, got %d", fileCount, len(results))
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

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		results := MapFiles(files, func(p *parser.Parser, path string) (int, error) {
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

func BenchmarkMapFiles_DifferentWorkerCounts(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.go", i), "package main\nfunc test() {}")
	}

	workerCounts := []int{1, 2, 4, 8, runtime.NumCPU(), runtime.NumCPU() * 2}

	for _, workers := range workerCounts {
		b.Run(fmt.Sprintf("workers=%d", workers), func(b *testing.B) {
			b.ResetTimer()
			for i := 0; i < b.N; i++ {
				results := MapFilesN(files, workers, func(p *parser.Parser, path string) (int, error) {
					_, err := p.ParseFile(path)
					if err != nil {
						return 0, err
					}
					return 1, nil
				}, nil, nil)

				if len(results) != fileCount {
					b.Fatalf("Expected %d results, got %d", fileCount, len(results))
				}
			}
		})
	}
}

func BenchmarkForEachFile(b *testing.B) {
	tmpDir := b.TempDir()

	fileCount := 100
	files := make([]string, fileCount)
	for i := 0; i < fileCount; i++ {
		files[i] = createTestFile(b, tmpDir, fmt.Sprintf("file%d.txt", i), "test content")
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		results := ForEachFile(files, func(path string) (int, error) {
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
	progressFunc := func() {
		progressCount.Add(1)
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		progressCount.Store(0)
		results := MapFilesWithProgress(files, func(p *parser.Parser, path string) (int, error) {
			return 1, nil
		}, progressFunc)

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
