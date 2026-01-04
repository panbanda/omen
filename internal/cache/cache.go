package cache

import (
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"time"

	"github.com/zeebo/blake3"
)

// Cache provides file-based caching for analysis results.
type Cache struct {
	dir     string
	ttl     time.Duration
	enabled bool
	maxSize int64 // Maximum cache size in bytes (0 = unlimited)
}

// Entry represents a cached analysis result.
type Entry struct {
	Hash      string    `json:"hash"`
	Timestamp time.Time `json:"timestamp"`
	Data      []byte    `json:"data"`
}

// Option is a functional option for configuring Cache.
type Option func(*Cache)

// WithMaxSize sets the maximum cache size in bytes.
// When exceeded, oldest entries are evicted before writing new ones.
func WithMaxSize(maxBytes int64) Option {
	return func(c *Cache) {
		c.maxSize = maxBytes
	}
}

// New creates a new cache instance.
func New(dir string, ttlHours int, enabled bool, opts ...Option) (*Cache, error) {
	if !enabled {
		return &Cache{enabled: false}, nil
	}

	// Ensure cache directory exists with secure permissions (owner-only access)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return nil, err
	}

	c := &Cache{
		dir:     dir,
		ttl:     time.Duration(ttlHours) * time.Hour,
		enabled: true,
	}
	for _, opt := range opts {
		opt(c)
	}
	return c, nil
}

// HashFile computes a BLAKE3 hash of a file's contents.
func HashFile(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	return HashBytes(data), nil
}

// HashBytes computes a BLAKE3 hash of bytes and returns it as a hex string.
func HashBytes(data []byte) string {
	hash := blake3.Sum256(data)
	return hex.EncodeToString(hash[:])
}

// Get retrieves a cached entry if it exists and is not expired.
func (c *Cache) Get(key string) ([]byte, bool) {
	if !c.enabled {
		return nil, false
	}

	path := c.keyPath(key)
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, false
	}

	var entry Entry
	if err := json.Unmarshal(data, &entry); err != nil {
		return nil, false
	}

	// Check TTL
	if time.Since(entry.Timestamp) > c.ttl {
		os.Remove(path)
		return nil, false
	}

	return entry.Data, true
}

// GetWithHash retrieves a cached entry only if the hash matches.
func (c *Cache) GetWithHash(key, hash string) ([]byte, bool) {
	if !c.enabled {
		return nil, false
	}

	path := c.keyPath(key)
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, false
	}

	var entry Entry
	if err := json.Unmarshal(data, &entry); err != nil {
		return nil, false
	}

	// Check hash match
	if entry.Hash != hash {
		return nil, false
	}

	// Check TTL
	if time.Since(entry.Timestamp) > c.ttl {
		os.Remove(path)
		return nil, false
	}

	return entry.Data, true
}

// Set stores data in the cache.
func (c *Cache) Set(key string, data []byte) error {
	if !c.enabled {
		return nil
	}

	entry := Entry{
		Timestamp: time.Now(),
		Data:      data,
	}

	entryData, err := json.Marshal(entry)
	if err != nil {
		return err
	}

	// Check size limit before writing
	if c.maxSize > 0 {
		if err := c.ensureSpace(int64(len(entryData))); err != nil {
			return err
		}
	}

	return os.WriteFile(c.keyPath(key), entryData, 0600)
}

// SetWithHash stores data in the cache with a hash for validation.
func (c *Cache) SetWithHash(key, hash string, data []byte) error {
	if !c.enabled {
		return nil
	}

	entry := Entry{
		Hash:      hash,
		Timestamp: time.Now(),
		Data:      data,
	}

	entryData, err := json.Marshal(entry)
	if err != nil {
		return err
	}

	// Check size limit before writing
	if c.maxSize > 0 {
		if err := c.ensureSpace(int64(len(entryData))); err != nil {
			return err
		}
	}

	return os.WriteFile(c.keyPath(key), entryData, 0600)
}

// Invalidate removes a cache entry.
func (c *Cache) Invalidate(key string) error {
	if !c.enabled {
		return nil
	}
	return os.Remove(c.keyPath(key))
}

// Clear removes all cache entries.
func (c *Cache) Clear() error {
	if !c.enabled {
		return nil
	}
	return os.RemoveAll(c.dir)
}

// keyPath converts a key to a filesystem path.
func (c *Cache) keyPath(key string) string {
	// Use BLAKE3 hash of key for filename to avoid path issues
	hash := blake3.Sum256([]byte(key))
	return filepath.Join(c.dir, hex.EncodeToString(hash[:])+".json")
}

// Stats returns cache statistics.
type Stats struct {
	Entries   int           `json:"entries"`
	TotalSize int64         `json:"total_size"`
	OldestAge time.Duration `json:"oldest_age"`
	NewestAge time.Duration `json:"newest_age"`
}

// cacheEntry holds info about a cache file for eviction purposes.
type cacheEntry struct {
	path    string
	modTime time.Time
	size    int64
}

// ensureSpace evicts oldest entries until there's room for newEntrySize bytes.
func (c *Cache) ensureSpace(newEntrySize int64) error {
	if c.maxSize <= 0 {
		return nil
	}

	var entries []cacheEntry
	var totalSize int64

	err := filepath.Walk(c.dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // Skip files we can't access
		}
		if info.IsDir() || filepath.Ext(path) != ".json" {
			return nil
		}
		entries = append(entries, cacheEntry{
			path:    path,
			modTime: info.ModTime(),
			size:    info.Size(),
		})
		totalSize += info.Size()
		return nil
	})
	if err != nil {
		return err
	}

	// Check if we need to evict
	if totalSize+newEntrySize <= c.maxSize {
		return nil
	}

	// Sort by modification time (oldest first)
	sort.Slice(entries, func(i, j int) bool {
		return entries[i].modTime.Before(entries[j].modTime)
	})

	// Evict oldest entries until we have enough space
	for _, entry := range entries {
		if totalSize+newEntrySize <= c.maxSize {
			break
		}
		if err := os.Remove(entry.path); err == nil {
			totalSize -= entry.size
		}
	}

	return nil
}

// GetStats returns statistics about the cache.
func (c *Cache) GetStats() (*Stats, error) {
	if !c.enabled {
		return &Stats{}, nil
	}

	stats := &Stats{}
	var oldest, newest time.Time

	err := filepath.Walk(c.dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() {
			return nil
		}
		if filepath.Ext(path) != ".json" {
			return nil
		}

		stats.Entries++
		stats.TotalSize += info.Size()

		modTime := info.ModTime()
		if oldest.IsZero() || modTime.Before(oldest) {
			oldest = modTime
		}
		if newest.IsZero() || modTime.After(newest) {
			newest = modTime
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	if !oldest.IsZero() {
		stats.OldestAge = time.Since(oldest)
	}
	if !newest.IsZero() {
		stats.NewestAge = time.Since(newest)
	}

	return stats, nil
}
