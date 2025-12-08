package testutil

import (
	"io/fs"
	"os"
	"path/filepath"
	"testing"
)

// WriteFile writes content to a file in the real filesystem.
func WriteFile(t *testing.T, path, content string) {
	t.Helper()
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		t.Fatalf("MkdirAll(%s) error: %v", dir, err)
	}
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("WriteFile(%s) error: %v", path, err)
	}
}

// ReadFile reads content from a file.
func ReadFile(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile(%s) error: %v", path, err)
	}
	return string(data)
}

// FileExists checks if a file exists.
func FileExists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}

// DirExists checks if a directory exists.
func DirExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.IsDir()
}

// TempDir creates a temporary directory and returns its path.
// The directory is automatically cleaned up when the test ends.
func TempDir(t *testing.T) string {
	t.Helper()
	dir, err := os.MkdirTemp("", "omen-test-*")
	if err != nil {
		t.Fatalf("MkdirTemp error: %v", err)
	}
	t.Cleanup(func() {
		os.RemoveAll(dir)
	})
	return dir
}

// CreateFileTree creates multiple files from a map of path -> content.
func CreateFileTree(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for name, content := range files {
		path := filepath.Join(root, name)
		WriteFile(t, path, content)
	}
}

// ListFiles returns all files in a directory recursively.
func ListFiles(t *testing.T, root string) []string {
	t.Helper()
	var files []string
	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if !d.IsDir() {
			files = append(files, path)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("WalkDir(%s) error: %v", root, err)
	}
	return files
}

// RepoRoot finds the repository root by searching for .git directory.
// Starts from the current working directory and walks up.
func RepoRoot(t *testing.T) string {
	t.Helper()
	dir, err := os.Getwd()
	if err != nil {
		t.Fatalf("Getwd error: %v", err)
	}

	for {
		gitPath := filepath.Join(dir, ".git")
		if info, err := os.Stat(gitPath); err == nil && info.IsDir() {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			t.Fatal("could not find repository root (.git directory)")
		}
		dir = parent
	}
}
