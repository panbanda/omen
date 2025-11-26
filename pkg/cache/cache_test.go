package cache

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestNew(t *testing.T) {
	tmpDir := t.TempDir()

	// Test enabled cache
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}
	if c == nil {
		t.Fatal("New() returned nil")
	}
	if !c.enabled {
		t.Error("cache should be enabled")
	}

	// Test disabled cache
	c, err = New("", 0, false)
	if err != nil {
		t.Fatalf("New() error for disabled cache: %v", err)
	}
	if c.enabled {
		t.Error("cache should be disabled")
	}
}

func TestNewCreatesDirectory(t *testing.T) {
	tmpDir := t.TempDir()
	cacheDir := filepath.Join(tmpDir, "nested", "cache", "dir")

	c, err := New(cacheDir, 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	if _, err := os.Stat(cacheDir); os.IsNotExist(err) {
		t.Error("New() should create cache directory")
	}

	_ = c
}

func TestSetAndGet(t *testing.T) {
	tmpDir := t.TempDir()
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	key := "test-key"
	data := []byte("test data content")

	// Set
	if err := c.Set(key, data); err != nil {
		t.Fatalf("Set() error: %v", err)
	}

	// Get
	got, ok := c.Get(key)
	if !ok {
		t.Fatal("Get() returned false for existing key")
	}
	if string(got) != string(data) {
		t.Errorf("Get() = %q, want %q", string(got), string(data))
	}
}

func TestGetNonExistent(t *testing.T) {
	tmpDir := t.TempDir()
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	_, ok := c.Get("nonexistent-key")
	if ok {
		t.Error("Get() should return false for non-existent key")
	}
}

func TestSetAndGetWithHash(t *testing.T) {
	tmpDir := t.TempDir()
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	key := "test-key"
	hash := "abc123"
	data := []byte("test data with hash")

	// Set with hash
	if err := c.SetWithHash(key, hash, data); err != nil {
		t.Fatalf("SetWithHash() error: %v", err)
	}

	// Get with matching hash
	got, ok := c.GetWithHash(key, hash)
	if !ok {
		t.Fatal("GetWithHash() returned false for matching hash")
	}
	if string(got) != string(data) {
		t.Errorf("GetWithHash() = %q, want %q", string(got), string(data))
	}

	// Get with non-matching hash
	_, ok = c.GetWithHash(key, "different-hash")
	if ok {
		t.Error("GetWithHash() should return false for non-matching hash")
	}
}

func TestInvalidate(t *testing.T) {
	tmpDir := t.TempDir()
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	key := "test-key"
	if err := c.Set(key, []byte("data")); err != nil {
		t.Fatalf("Set() error: %v", err)
	}

	// Verify it exists
	if _, ok := c.Get(key); !ok {
		t.Fatal("Key should exist before invalidation")
	}

	// Invalidate
	if err := c.Invalidate(key); err != nil {
		t.Fatalf("Invalidate() error: %v", err)
	}

	// Verify it's gone
	if _, ok := c.Get(key); ok {
		t.Error("Key should not exist after invalidation")
	}
}

func TestClear(t *testing.T) {
	tmpDir := t.TempDir()
	cacheDir := filepath.Join(tmpDir, "cache")
	c, err := New(cacheDir, 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	// Add some entries
	for i := 0; i < 5; i++ {
		key := string(rune('a' + i))
		if err := c.Set(key, []byte("data")); err != nil {
			t.Fatalf("Set() error: %v", err)
		}
	}

	// Clear
	if err := c.Clear(); err != nil {
		t.Fatalf("Clear() error: %v", err)
	}

	// Verify cache directory is gone
	if _, err := os.Stat(cacheDir); !os.IsNotExist(err) {
		t.Error("Clear() should remove cache directory")
	}
}

func TestDisabledCache(t *testing.T) {
	c, err := New("", 0, false)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	// All operations should be no-ops on disabled cache
	if err := c.Set("key", []byte("data")); err != nil {
		t.Errorf("Set() on disabled cache should not error: %v", err)
	}

	if _, ok := c.Get("key"); ok {
		t.Error("Get() on disabled cache should return false")
	}

	if err := c.SetWithHash("key", "hash", []byte("data")); err != nil {
		t.Errorf("SetWithHash() on disabled cache should not error: %v", err)
	}

	if _, ok := c.GetWithHash("key", "hash"); ok {
		t.Error("GetWithHash() on disabled cache should return false")
	}

	if err := c.Invalidate("key"); err != nil {
		t.Errorf("Invalidate() on disabled cache should not error: %v", err)
	}

	if err := c.Clear(); err != nil {
		t.Errorf("Clear() on disabled cache should not error: %v", err)
	}
}

func TestHashFile(t *testing.T) {
	tmpDir := t.TempDir()
	filePath := filepath.Join(tmpDir, "test.txt")

	content := "test content for hashing"
	if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to create test file: %v", err)
	}

	hash1, err := HashFile(filePath)
	if err != nil {
		t.Fatalf("HashFile() error: %v", err)
	}

	if hash1 == "" {
		t.Error("HashFile() returned empty hash")
	}

	// Same content should produce same hash
	hash2, err := HashFile(filePath)
	if err != nil {
		t.Fatalf("HashFile() error: %v", err)
	}

	if hash1 != hash2 {
		t.Error("HashFile() should return consistent hashes")
	}

	// Different content should produce different hash
	if err := os.WriteFile(filePath, []byte("different content"), 0644); err != nil {
		t.Fatalf("Failed to update test file: %v", err)
	}

	hash3, err := HashFile(filePath)
	if err != nil {
		t.Fatalf("HashFile() error: %v", err)
	}

	if hash1 == hash3 {
		t.Error("HashFile() should return different hashes for different content")
	}
}

func TestHashFileNonExistent(t *testing.T) {
	_, err := HashFile("/nonexistent/path/file.txt")
	if err == nil {
		t.Error("HashFile() should return error for non-existent file")
	}
}

func TestHashBytes(t *testing.T) {
	data1 := []byte("hello world")
	data2 := []byte("hello world")
	data3 := []byte("different")

	hash1 := HashBytes(data1)
	hash2 := HashBytes(data2)
	hash3 := HashBytes(data3)

	if hash1 == "" {
		t.Error("HashBytes() returned empty hash")
	}

	if hash1 != hash2 {
		t.Error("HashBytes() should return consistent hashes for same content")
	}

	if hash1 == hash3 {
		t.Error("HashBytes() should return different hashes for different content")
	}
}

func TestGetStats(t *testing.T) {
	tmpDir := t.TempDir()
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	// Empty cache
	stats, err := c.GetStats()
	if err != nil {
		t.Fatalf("GetStats() error: %v", err)
	}

	if stats.Entries != 0 {
		t.Errorf("Empty cache should have 0 entries, got %d", stats.Entries)
	}

	// Add entries
	for i := 0; i < 3; i++ {
		key := string(rune('a' + i))
		if err := c.Set(key, []byte("data")); err != nil {
			t.Fatalf("Set() error: %v", err)
		}
	}

	stats, err = c.GetStats()
	if err != nil {
		t.Fatalf("GetStats() error: %v", err)
	}

	if stats.Entries != 3 {
		t.Errorf("Cache should have 3 entries, got %d", stats.Entries)
	}

	if stats.TotalSize <= 0 {
		t.Error("TotalSize should be positive")
	}
}

func TestGetStatsDisabled(t *testing.T) {
	c, _ := New("", 0, false)

	stats, err := c.GetStats()
	if err != nil {
		t.Fatalf("GetStats() error: %v", err)
	}

	if stats.Entries != 0 {
		t.Errorf("Disabled cache stats should have 0 entries, got %d", stats.Entries)
	}
}

func TestTTLExpiration(t *testing.T) {
	if testing.Short() {
		t.Skip("Skipping TTL test in short mode")
	}

	tmpDir := t.TempDir()
	// Create cache with 1 second TTL (minimum useful for testing)
	c := &Cache{
		dir:     filepath.Join(tmpDir, "cache"),
		ttl:     1 * time.Second,
		enabled: true,
	}
	os.MkdirAll(c.dir, 0755)

	key := "test-key"
	data := []byte("test data")

	if err := c.Set(key, data); err != nil {
		t.Fatalf("Set() error: %v", err)
	}

	// Should be available immediately
	if _, ok := c.Get(key); !ok {
		t.Error("Get() should return data before TTL expires")
	}

	// Wait for TTL to expire
	time.Sleep(2 * time.Second)

	// Should be expired now
	if _, ok := c.Get(key); ok {
		t.Error("Get() should return false after TTL expires")
	}
}

func TestKeyPath(t *testing.T) {
	tmpDir := t.TempDir()
	c, _ := New(filepath.Join(tmpDir, "cache"), 24, true)

	// Different keys should produce different paths
	path1 := c.keyPath("key1")
	path2 := c.keyPath("key2")
	path3 := c.keyPath("key1") // Same as path1

	if path1 == path2 {
		t.Error("Different keys should produce different paths")
	}

	if path1 != path3 {
		t.Error("Same keys should produce same paths")
	}

	// Path should end with .json
	if filepath.Ext(path1) != ".json" {
		t.Errorf("Key path should end with .json, got %s", path1)
	}

	// Path should be in cache directory
	if filepath.Dir(path1) != c.dir {
		t.Errorf("Key path should be in cache directory")
	}
}

func TestSpecialCharactersInKey(t *testing.T) {
	tmpDir := t.TempDir()
	c, err := New(filepath.Join(tmpDir, "cache"), 24, true)
	if err != nil {
		t.Fatalf("New() error: %v", err)
	}

	// Keys with special characters should work
	keys := []string{
		"/path/to/file.go",
		"file:with:colons",
		"file with spaces",
		"unicode/文件/test",
	}

	for _, key := range keys {
		t.Run(key, func(t *testing.T) {
			data := []byte("data for " + key)

			if err := c.Set(key, data); err != nil {
				t.Errorf("Set(%q) error: %v", key, err)
				return
			}

			got, ok := c.Get(key)
			if !ok {
				t.Errorf("Get(%q) returned false", key)
				return
			}

			if string(got) != string(data) {
				t.Errorf("Get(%q) = %q, want %q", key, string(got), string(data))
			}
		})
	}
}
