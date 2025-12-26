package analysis

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/internal/cache"
	"github.com/panbanda/omen/pkg/config"
)

// TestServiceWithCache verifies cache integration
func TestServiceWithCache(t *testing.T) {
	// Create temp dir for cache
	cacheDir := t.TempDir()

	// Create test cache
	testCache, err := cache.New(cacheDir, 24, true)
	if err != nil {
		t.Fatalf("Failed to create cache: %v", err)
	}

	// Create service with cache
	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg), WithCache(testCache))

	if svc.cache == nil {
		t.Error("Cache should be set on service")
	}
}

// TestServiceWithoutCache verifies service works without cache
func TestServiceWithoutCache(t *testing.T) {
	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg))

	if svc.cache != nil {
		t.Error("Cache should be nil by default")
	}
}

// TestCachedComplexityAnalysis_CachePopulated verifies cache is populated after analysis
func TestCachedComplexityAnalysis_CachePopulated(t *testing.T) {
	// Create temp dir for cache and test files
	cacheDir := t.TempDir()
	testDir := t.TempDir()

	// Create test file
	testFile := filepath.Join(testDir, "test.go")
	if err := os.WriteFile(testFile, []byte("package main\nfunc main() {}\n"), 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	// Create cache
	testCache, err := cache.New(cacheDir, 24, true)
	if err != nil {
		t.Fatalf("Failed to create cache: %v", err)
	}

	// Verify cache is empty before analysis
	statsBefore, err := testCache.GetStats()
	if err != nil {
		t.Fatalf("Failed to get cache stats: %v", err)
	}
	if statsBefore.Entries != 0 {
		t.Fatalf("Cache should be empty before analysis, got %d entries", statsBefore.Entries)
	}

	// Create service with cache
	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg), WithCache(testCache))

	ctx := context.Background()
	files := []string{testFile}

	// Run analysis
	result, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("Analysis failed: %v", err)
	}
	if result == nil {
		t.Fatal("Result should not be nil")
	}

	// Verify cache was populated
	statsAfter, err := testCache.GetStats()
	if err != nil {
		t.Fatalf("Failed to get cache stats: %v", err)
	}
	if statsAfter.Entries == 0 {
		t.Error("Cache should have entries after analysis, but has 0")
	}
}

// TestCacheKeyGeneration verifies unique keys are generated
func TestCacheKeyGeneration(t *testing.T) {
	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg))

	files1 := []string{"a.go", "b.go"}
	files2 := []string{"b.go", "a.go"} // Same files, different order
	files3 := []string{"a.go", "c.go"} // Different files

	key1 := svc.cacheKey("complexity", files1, nil)
	key2 := svc.cacheKey("complexity", files2, nil)
	key3 := svc.cacheKey("complexity", files3, nil)

	// Same files in different order should produce same key (sorted)
	if key1 != key2 {
		t.Errorf("Keys should be equal for same files: %s vs %s", key1, key2)
	}

	// Different files should produce different key
	if key1 == key3 {
		t.Errorf("Keys should be different for different files: %s vs %s", key1, key3)
	}
}

// TestCacheKeyIncludesOptions verifies options are part of cache key
func TestCacheKeyIncludesOptions(t *testing.T) {
	cacheDir := t.TempDir()
	testDir := t.TempDir()

	// Create a file that will be analyzed
	testFile := filepath.Join(testDir, "test.go")
	content := []byte("package main\nfunc main() {\n\tif true { }\n}\n")
	if err := os.WriteFile(testFile, content, 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	testCache, err := cache.New(cacheDir, 24, true)
	if err != nil {
		t.Fatalf("Failed to create cache: %v", err)
	}

	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg), WithCache(testCache))

	ctx := context.Background()
	files := []string{testFile}

	// First call with default options
	result1, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{
		CyclomaticThreshold: 10,
	})
	if err != nil {
		t.Fatalf("First analysis failed: %v", err)
	}

	// Second call with different options - should NOT hit cache
	result2, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{
		CyclomaticThreshold: 5,
	})
	if err != nil {
		t.Fatalf("Second analysis failed: %v", err)
	}

	// Check cache has 2 entries (one for each option set)
	stats, err := testCache.GetStats()
	if err != nil {
		t.Fatalf("Failed to get cache stats: %v", err)
	}

	// This test will FAIL because options are not included in cache key
	// Currently only 1 entry exists because same key is reused
	if stats.Entries != 2 {
		t.Errorf("Cache should have 2 entries (one per option set), got %d", stats.Entries)
	}

	_ = result1
	_ = result2
}

// BenchmarkComplexityWithCache measures cache hit performance
func BenchmarkComplexityWithCache(b *testing.B) {
	cacheDir := b.TempDir()
	testDir := b.TempDir()

	// Create several test files
	for i := 0; i < 10; i++ {
		testFile := filepath.Join(testDir, fmt.Sprintf("test%d.go", i))
		content := []byte("package main\nfunc foo() {\n\tif true { println(1) }\n\tfor i := 0; i < 10; i++ { println(i) }\n}\n")
		if err := os.WriteFile(testFile, content, 0644); err != nil {
			b.Fatalf("Failed to create test file: %v", err)
		}
	}

	testCache, err := cache.New(cacheDir, 24, true)
	if err != nil {
		b.Fatalf("Failed to create cache: %v", err)
	}

	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg), WithCache(testCache))

	ctx := context.Background()

	// Get list of files
	files, err := filepath.Glob(filepath.Join(testDir, "*.go"))
	if err != nil {
		b.Fatalf("Failed to glob files: %v", err)
	}

	// Warm the cache
	_, err = svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		b.Fatalf("Warmup failed: %v", err)
	}

	b.ResetTimer()

	// Benchmark cache hits
	for i := 0; i < b.N; i++ {
		_, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
		if err != nil {
			b.Fatalf("Analysis failed: %v", err)
		}
	}
}

// BenchmarkComplexityWithoutCache measures no-cache performance
func BenchmarkComplexityWithoutCache(b *testing.B) {
	testDir := b.TempDir()

	// Create several test files
	for i := 0; i < 10; i++ {
		testFile := filepath.Join(testDir, fmt.Sprintf("test%d.go", i))
		content := []byte("package main\nfunc foo() {\n\tif true { println(1) }\n\tfor i := 0; i < 10; i++ { println(i) }\n}\n")
		if err := os.WriteFile(testFile, content, 0644); err != nil {
			b.Fatalf("Failed to create test file: %v", err)
		}
	}

	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg)) // No cache

	ctx := context.Background()

	// Get list of files
	files, err := filepath.Glob(filepath.Join(testDir, "*.go"))
	if err != nil {
		b.Fatalf("Failed to glob files: %v", err)
	}

	b.ResetTimer()

	for i := 0; i < b.N; i++ {
		_, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
		if err != nil {
			b.Fatalf("Analysis failed: %v", err)
		}
	}
}

// TestCacheInvalidation verifies cache is invalidated on file change
func TestCacheInvalidation(t *testing.T) {
	cacheDir := t.TempDir()
	testDir := t.TempDir()

	// Initial file with simple function (cyclomatic complexity = 1)
	testFile := filepath.Join(testDir, "test.go")
	simpleContent := []byte("package main\nfunc main() {}\n")
	if err := os.WriteFile(testFile, simpleContent, 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	testCache, err := cache.New(cacheDir, 24, true)
	if err != nil {
		t.Fatalf("Failed to create cache: %v", err)
	}

	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg), WithCache(testCache))

	ctx := context.Background()
	files := []string{testFile}

	// First analysis - simple function, cyclomatic = 1
	result1, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("First analysis failed: %v", err)
	}
	if len(result1.Files) == 0 || len(result1.Files[0].Functions) == 0 {
		t.Fatal("Expected at least one function in analysis")
	}
	complexityBefore := result1.Files[0].Functions[0].Metrics.Cyclomatic

	// Modify file to add complexity (if statement adds 1 to cyclomatic)
	complexContent := []byte("package main\nfunc main() {\n\tif true { println(1) }\n\tif false { println(2) }\n}\n")
	if err := os.WriteFile(testFile, complexContent, 0644); err != nil {
		t.Fatalf("Failed to modify test file: %v", err)
	}

	// Second analysis should detect change and return NEW complexity
	result2, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("Second analysis failed: %v", err)
	}
	if len(result2.Files) == 0 || len(result2.Files[0].Functions) == 0 {
		t.Fatal("Expected at least one function in second analysis")
	}
	complexityAfter := result2.Files[0].Functions[0].Metrics.Cyclomatic

	// This test will FAIL with current implementation because cache returns stale data
	// complexityBefore should be 1, complexityAfter should be 3 (1 + 2 ifs)
	if complexityBefore == complexityAfter {
		t.Errorf("Cache returned stale data! Complexity before=%d, after=%d (should differ after file change)",
			complexityBefore, complexityAfter)
	}
}
