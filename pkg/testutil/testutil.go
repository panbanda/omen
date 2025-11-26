package testutil

import (
	"path/filepath"
	"testing"

	"github.com/spf13/afero"
)

// MemFS creates an in-memory filesystem for testing.
func MemFS() afero.Fs {
	return afero.NewMemMapFs()
}

// WriteFile writes content to a file in the given filesystem.
func WriteFile(t *testing.T, fs afero.Fs, path, content string) {
	t.Helper()
	dir := filepath.Dir(path)
	if err := fs.MkdirAll(dir, 0755); err != nil {
		t.Fatalf("MkdirAll(%s) error: %v", dir, err)
	}
	if err := afero.WriteFile(fs, path, []byte(content), 0644); err != nil {
		t.Fatalf("WriteFile(%s) error: %v", path, err)
	}
}

// ReadFile reads content from a file in the given filesystem.
func ReadFile(t *testing.T, fs afero.Fs, path string) string {
	t.Helper()
	data, err := afero.ReadFile(fs, path)
	if err != nil {
		t.Fatalf("ReadFile(%s) error: %v", path, err)
	}
	return string(data)
}

// FileExists checks if a file exists in the filesystem.
func FileExists(fs afero.Fs, path string) bool {
	exists, _ := afero.Exists(fs, path)
	return exists
}

// DirExists checks if a directory exists in the filesystem.
func DirExists(fs afero.Fs, path string) bool {
	exists, _ := afero.DirExists(fs, path)
	return exists
}

// TempDir creates a temporary directory in the filesystem and returns its path.
// For MemMapFs, this just creates a directory with a unique name.
func TempDir(t *testing.T, fs afero.Fs, prefix string) string {
	t.Helper()
	dir := filepath.Join("/tmp", prefix+t.Name())
	if err := fs.MkdirAll(dir, 0755); err != nil {
		t.Fatalf("MkdirAll(%s) error: %v", dir, err)
	}
	return dir
}

// CreateFileTree creates multiple files from a map of path -> content.
func CreateFileTree(t *testing.T, fs afero.Fs, root string, files map[string]string) {
	t.Helper()
	for name, content := range files {
		path := filepath.Join(root, name)
		WriteFile(t, fs, path, content)
	}
}

// ListFiles returns all files in a directory recursively.
func ListFiles(t *testing.T, fs afero.Fs, root string) []string {
	t.Helper()
	var files []string
	err := afero.Walk(fs, root, func(path string, info afero.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if !info.IsDir() {
			files = append(files, path)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("Walk(%s) error: %v", root, err)
	}
	return files
}
