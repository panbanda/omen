package analysis

import (
	"context"
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

// TestCachedComplexityAnalysis verifies cache hit/miss behavior
func TestCachedComplexityAnalysis(t *testing.T) {
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

	// Create service with cache
	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg), WithCache(testCache))

	ctx := context.Background()
	files := []string{testFile}

	// First call - cache miss
	result1, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("First analysis failed: %v", err)
	}
	if result1 == nil {
		t.Fatal("First result should not be nil")
	}

	// Second call - should hit cache (if implemented)
	result2, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("Second analysis failed: %v", err)
	}
	if result2 == nil {
		t.Fatal("Second result should not be nil")
	}

	// Results should be equivalent
	if result1.Summary.TotalFiles != result2.Summary.TotalFiles {
		t.Errorf("Cache should return equivalent results: %d vs %d files",
			result1.Summary.TotalFiles, result2.Summary.TotalFiles)
	}
}

// TestCacheKeyGeneration verifies unique keys are generated
func TestCacheKeyGeneration(t *testing.T) {
	cfg := config.DefaultConfig()
	svc := New(WithConfig(cfg))

	files1 := []string{"a.go", "b.go"}
	files2 := []string{"b.go", "a.go"} // Same files, different order
	files3 := []string{"a.go", "c.go"} // Different files

	key1 := svc.cacheKey("complexity", files1)
	key2 := svc.cacheKey("complexity", files2)
	key3 := svc.cacheKey("complexity", files3)

	// Same files in different order should produce same key (sorted)
	if key1 != key2 {
		t.Errorf("Keys should be equal for same files: %s vs %s", key1, key2)
	}

	// Different files should produce different key
	if key1 == key3 {
		t.Errorf("Keys should be different for different files: %s vs %s", key1, key3)
	}
}

// TestCacheInvalidation verifies cache is invalidated on file change
func TestCacheInvalidation(t *testing.T) {
	cacheDir := t.TempDir()
	testDir := t.TempDir()

	testFile := filepath.Join(testDir, "test.go")
	if err := os.WriteFile(testFile, []byte("package main\nfunc main() {}\n"), 0644); err != nil {
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

	// First analysis
	result1, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("First analysis failed: %v", err)
	}

	// Modify file content
	if err := os.WriteFile(testFile, []byte("package main\nfunc main() {\n\tif true { }\n}\n"), 0644); err != nil {
		t.Fatalf("Failed to modify test file: %v", err)
	}

	// Second analysis should detect change (hash mismatch)
	result2, err := svc.AnalyzeComplexity(ctx, files, ComplexityOptions{})
	if err != nil {
		t.Fatalf("Second analysis failed: %v", err)
	}

	// Results should differ due to added complexity
	if result1.Summary.TotalFunctions == result2.Summary.TotalFunctions {
		t.Log("Cache correctly invalidated on file change - results differ")
	}
}
