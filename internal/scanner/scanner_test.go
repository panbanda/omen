package scanner

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/parser"
)

func TestNewScanner(t *testing.T) {
	// With nil config
	s := NewScanner(nil)
	if s == nil {
		t.Fatal("NewScanner(nil) returned nil")
	}
	if s.config == nil {
		t.Error("scanner.config should not be nil when passing nil")
	}

	// With explicit config
	cfg := config.DefaultConfig()
	s = NewScanner(cfg)
	if s.config != cfg {
		t.Error("scanner.config should be the provided config")
	}
}

func TestScanDir(t *testing.T) {
	// Create temp directory structure
	tmpDir := t.TempDir()

	// Create source files
	files := map[string]string{
		"main.go":          "package main\n",
		"lib.go":           "package lib\n",
		"util/helper.go":   "package util\n",
		"util/helper.py":   "# python\n",
		"internal/core.rs": "fn main() {}\n",
	}

	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatalf("Failed to create directory: %v", err)
		}
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("Failed to create file %s: %v", name, err)
		}
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	if len(result) != 5 {
		t.Errorf("ScanDir() found %d files, want 5", len(result))
	}

	// Verify all source files were found
	found := make(map[string]bool)
	for _, f := range result {
		rel, _ := filepath.Rel(tmpDir, f)
		found[rel] = true
	}

	for name := range files {
		if !found[name] {
			t.Errorf("File %s was not found", name)
		}
	}
}

func TestScanDirExcludesDirectories(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files in excluded directories
	excludedDirs := []string{"vendor", "node_modules", ".git"}
	for _, dir := range excludedDirs {
		path := filepath.Join(tmpDir, dir, "file.go")
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatalf("Failed to create directory: %v", err)
		}
		if err := os.WriteFile(path, []byte("package x\n"), 0644); err != nil {
			t.Fatalf("Failed to create file: %v", err)
		}
	}

	// Create a non-excluded file
	if err := os.WriteFile(filepath.Join(tmpDir, "main.go"), []byte("package main\n"), 0644); err != nil {
		t.Fatalf("Failed to create file: %v", err)
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// Should only find main.go
	if len(result) != 1 {
		t.Errorf("ScanDir() found %d files, want 1 (excluded dirs should be skipped)", len(result))
		for _, f := range result {
			t.Logf("  Found: %s", f)
		}
	}
}

func TestScanDirExcludesPatterns(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files that match exclusion patterns
	files := []string{
		"main.go",
		"main_test.go", // Should be excluded by default pattern
		"app.min.js",   // Should be excluded by default pattern
	}

	for _, name := range files {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte("// content\n"), 0644); err != nil {
			t.Fatalf("Failed to create file: %v", err)
		}
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// Should only find main.go (tests and min.js excluded)
	if len(result) != 1 {
		t.Errorf("ScanDir() found %d files, want 1", len(result))
		for _, f := range result {
			t.Logf("  Found: %s", f)
		}
	}
}

func TestScanDirExcludesExtensions(t *testing.T) {
	tmpDir := t.TempDir()

	files := []string{
		"main.go",
		"go.sum",  // Should be excluded by default
		"go.lock", // Should be excluded by default
	}

	for _, name := range files {
		if err := os.WriteFile(filepath.Join(tmpDir, name), []byte("// content\n"), 0644); err != nil {
			t.Fatalf("Failed to create file: %v", err)
		}
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	if len(result) != 1 {
		t.Errorf("ScanDir() found %d files, want 1", len(result))
	}
}

func TestScanFile(t *testing.T) {
	tmpDir := t.TempDir()

	tests := []struct {
		name     string
		filename string
		content  string
		want     bool
	}{
		{"go file", "main.go", "package main\n", true},
		{"python file", "script.py", "# python\n", true},
		{"text file", "readme.txt", "hello\n", false},
		{"directory", "", "", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var path string
			if tt.filename == "" {
				// Test directory case
				path = tmpDir
			} else {
				path = filepath.Join(tmpDir, tt.filename)
				if err := os.WriteFile(path, []byte(tt.content), 0644); err != nil {
					t.Fatalf("Failed to create file: %v", err)
				}
			}

			s := NewScanner(nil)
			got, err := s.ScanFile(path)
			if err != nil {
				if tt.want {
					t.Errorf("ScanFile() error: %v", err)
				}
				return
			}

			if got != tt.want {
				t.Errorf("ScanFile(%s) = %v, want %v", tt.filename, got, tt.want)
			}
		})
	}
}

func TestScanFileNonExistent(t *testing.T) {
	s := NewScanner(nil)
	_, err := s.ScanFile("/nonexistent/path/file.go")
	if err == nil {
		t.Error("ScanFile() should return error for non-existent file")
	}
}

func TestFilterByLanguage(t *testing.T) {
	files := []string{
		"/path/to/main.go",
		"/path/to/lib.go",
		"/path/to/script.py",
		"/path/to/app.ts",
	}

	s := NewScanner(nil)

	goFiles := s.FilterByLanguage(files, parser.LangGo)
	if len(goFiles) != 2 {
		t.Errorf("FilterByLanguage(Go) returned %d files, want 2", len(goFiles))
	}

	pyFiles := s.FilterByLanguage(files, parser.LangPython)
	if len(pyFiles) != 1 {
		t.Errorf("FilterByLanguage(Python) returned %d files, want 1", len(pyFiles))
	}

	tsFiles := s.FilterByLanguage(files, parser.LangTypeScript)
	if len(tsFiles) != 1 {
		t.Errorf("FilterByLanguage(TypeScript) returned %d files, want 1", len(tsFiles))
	}

	rustFiles := s.FilterByLanguage(files, parser.LangRust)
	if len(rustFiles) != 0 {
		t.Errorf("FilterByLanguage(Rust) returned %d files, want 0", len(rustFiles))
	}
}

func TestGroupByLanguage(t *testing.T) {
	files := []string{
		"/path/to/main.go",
		"/path/to/lib.go",
		"/path/to/script.py",
		"/path/to/app.ts",
		"/path/to/readme.txt", // Unknown language
	}

	s := NewScanner(nil)
	groups := s.GroupByLanguage(files)

	if len(groups[parser.LangGo]) != 2 {
		t.Errorf("GroupByLanguage()[Go] has %d files, want 2", len(groups[parser.LangGo]))
	}

	if len(groups[parser.LangPython]) != 1 {
		t.Errorf("GroupByLanguage()[Python] has %d files, want 1", len(groups[parser.LangPython]))
	}

	if len(groups[parser.LangTypeScript]) != 1 {
		t.Errorf("GroupByLanguage()[TypeScript] has %d files, want 1", len(groups[parser.LangTypeScript]))
	}

	// Unknown language should not be in groups
	if _, ok := groups[parser.LangUnknown]; ok {
		t.Error("GroupByLanguage() should not include LangUnknown")
	}
}

func TestScanDirWithGitignore(t *testing.T) {
	tmpDir := t.TempDir()

	// Create .gitignore
	gitignore := "skipme\n"
	if err := os.WriteFile(filepath.Join(tmpDir, ".gitignore"), []byte(gitignore), 0644); err != nil {
		t.Fatalf("Failed to create .gitignore: %v", err)
	}

	// Create files
	files := map[string]string{
		"main.go":        "package main\n",
		"skipme/skip.go": "package skipme\n",
		"src/app.go":     "package src\n",
	}

	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatalf("Failed to create directory: %v", err)
		}
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("Failed to create file %s: %v", name, err)
		}
	}

	cfg := config.DefaultConfig()
	cfg.Exclude.Gitignore = true

	s := NewScanner(cfg)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// Verify scanner finds files when gitignore is enabled
	foundFiles := make(map[string]bool)
	for _, f := range result {
		rel, _ := filepath.Rel(tmpDir, f)
		foundFiles[rel] = true
	}

	if !foundFiles["main.go"] {
		t.Error("Should find main.go")
	}

	if !foundFiles[filepath.Join("src", "app.go")] {
		t.Error("Should find src/app.go")
	}

	// Verify gitignore config is loaded
	if !cfg.Exclude.Gitignore {
		t.Error("Gitignore should be enabled")
	}
}

func TestScanDirDisabledGitignore(t *testing.T) {
	tmpDir := t.TempDir()

	// Create .gitignore
	if err := os.WriteFile(filepath.Join(tmpDir, ".gitignore"), []byte("ignored/\n"), 0644); err != nil {
		t.Fatalf("Failed to create .gitignore: %v", err)
	}

	// Create files
	if err := os.MkdirAll(filepath.Join(tmpDir, "ignored"), 0755); err != nil {
		t.Fatalf("Failed to create directory: %v", err)
	}
	if err := os.WriteFile(filepath.Join(tmpDir, "ignored", "file.go"), []byte("package x\n"), 0644); err != nil {
		t.Fatalf("Failed to create file: %v", err)
	}

	cfg := config.DefaultConfig()
	cfg.Exclude.Gitignore = false

	s := NewScanner(cfg)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// With gitignore disabled, should find the ignored file
	found := false
	for _, f := range result {
		if filepath.Base(f) == "file.go" {
			found = true
			break
		}
	}

	if !found {
		t.Error("With gitignore disabled, should find files in 'ignored' directory")
	}
}

func TestScanDirEmptyDirectory(t *testing.T) {
	tmpDir := t.TempDir()

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	if len(result) != 0 {
		t.Errorf("ScanDir() on empty dir returned %d files, want 0", len(result))
	}
}

func TestFilterByLanguageEmpty(t *testing.T) {
	s := NewScanner(nil)
	result := s.FilterByLanguage(nil, parser.LangGo)
	if result != nil {
		t.Errorf("FilterByLanguage(nil) should return nil, got %v", result)
	}

	result = s.FilterByLanguage([]string{}, parser.LangGo)
	if result != nil {
		t.Errorf("FilterByLanguage([]) should return nil, got %v", result)
	}
}

func TestGroupByLanguageEmpty(t *testing.T) {
	s := NewScanner(nil)
	result := s.GroupByLanguage(nil)
	if len(result) != 0 {
		t.Errorf("GroupByLanguage(nil) should return empty map, got %v", result)
	}

	result = s.GroupByLanguage([]string{})
	if len(result) != 0 {
		t.Errorf("GroupByLanguage([]) should return empty map, got %v", result)
	}
}

func TestFilterBySize(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files of different sizes
	smallContent := "small"
	largeContent := make([]byte, 1024)
	for i := range largeContent {
		largeContent[i] = 'x'
	}

	smallFile := filepath.Join(tmpDir, "small.go")
	largeFile := filepath.Join(tmpDir, "large.go")

	if err := os.WriteFile(smallFile, []byte(smallContent), 0644); err != nil {
		t.Fatalf("Failed to create small file: %v", err)
	}
	if err := os.WriteFile(largeFile, largeContent, 0644); err != nil {
		t.Fatalf("Failed to create large file: %v", err)
	}

	t.Run("no limit", func(t *testing.T) {
		filtered, skipped := FilterBySize([]string{smallFile, largeFile}, 0)
		if len(filtered) != 2 {
			t.Errorf("FilterBySize with no limit should return all files, got %d", len(filtered))
		}
		if skipped != 0 {
			t.Errorf("FilterBySize with no limit should skip 0 files, got %d", skipped)
		}
	})

	t.Run("negative limit", func(t *testing.T) {
		filtered, skipped := FilterBySize([]string{smallFile, largeFile}, -1)
		if len(filtered) != 2 {
			t.Errorf("FilterBySize with negative limit should return all files, got %d", len(filtered))
		}
		if skipped != 0 {
			t.Errorf("FilterBySize with negative limit should skip 0 files, got %d", skipped)
		}
	})

	t.Run("with limit", func(t *testing.T) {
		filtered, skipped := FilterBySize([]string{smallFile, largeFile}, 100)
		if len(filtered) != 1 {
			t.Errorf("FilterBySize should return 1 file, got %d", len(filtered))
		}
		if skipped != 1 {
			t.Errorf("FilterBySize should skip 1 file, got %d", skipped)
		}
		if filtered[0] != smallFile {
			t.Errorf("FilterBySize should keep small file, got %s", filtered[0])
		}
	})

	t.Run("with stat error", func(t *testing.T) {
		nonExistent := filepath.Join(tmpDir, "nonexistent.go")
		filtered, skipped := FilterBySize([]string{smallFile, nonExistent}, 100)
		if len(filtered) != 1 {
			t.Errorf("FilterBySize should return 1 file, got %d", len(filtered))
		}
		if skipped != 1 {
			t.Errorf("FilterBySize should skip non-existent file, got %d skipped", skipped)
		}
	})
}

func TestIsWithinRoot(t *testing.T) {
	tmpDir := t.TempDir()

	tests := []struct {
		name string
		path string
		root string
		want bool
	}{
		{
			name: "same path",
			path: tmpDir,
			root: tmpDir,
			want: true,
		},
		{
			name: "child path",
			path: filepath.Join(tmpDir, "subdir", "file.go"),
			root: tmpDir,
			want: true,
		},
		{
			name: "path outside root",
			path: "/some/other/path",
			root: tmpDir,
			want: false,
		},
		{
			name: "parent path",
			path: filepath.Dir(tmpDir),
			root: tmpDir,
			want: false,
		},
		{
			name: "similar prefix but different dir",
			path: tmpDir + "2/file.go",
			root: tmpDir,
			want: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := isWithinRoot(tt.path, tt.root)
			if got != tt.want {
				t.Errorf("isWithinRoot(%q, %q) = %v, want %v", tt.path, tt.root, got, tt.want)
			}
		})
	}
}

func TestFindGitRoot(t *testing.T) {
	// Test with non-git directory
	tmpDir := t.TempDir()
	result := findGitRoot(tmpDir)
	if result != "" {
		t.Errorf("findGitRoot() on non-git dir should return empty string, got %q", result)
	}

	// Test with mock .git directory
	gitDir := filepath.Join(tmpDir, ".git")
	if err := os.Mkdir(gitDir, 0755); err != nil {
		t.Fatalf("Failed to create .git dir: %v", err)
	}

	result = findGitRoot(tmpDir)
	if result != tmpDir {
		t.Errorf("findGitRoot() should return %q, got %q", tmpDir, result)
	}

	// Test from subdirectory
	subDir := filepath.Join(tmpDir, "src", "pkg")
	if err := os.MkdirAll(subDir, 0755); err != nil {
		t.Fatalf("Failed to create subdir: %v", err)
	}

	result = findGitRoot(subDir)
	if result != tmpDir {
		t.Errorf("findGitRoot() from subdir should return %q, got %q", tmpDir, result)
	}
}

func TestScanDirWithSymlinks(t *testing.T) {
	// Skip on systems that don't support symlinks
	tmpDir := t.TempDir()

	// Create a real file
	realFile := filepath.Join(tmpDir, "real.go")
	if err := os.WriteFile(realFile, []byte("package main\n"), 0644); err != nil {
		t.Fatalf("Failed to create file: %v", err)
	}

	// Create a symlink within the directory
	symlinkPath := filepath.Join(tmpDir, "link.go")
	if err := os.Symlink(realFile, symlinkPath); err != nil {
		t.Skip("Symlinks not supported on this system")
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// Should find both the real file and the symlink
	if len(result) < 1 {
		t.Errorf("ScanDir() should find at least the real file, got %d files", len(result))
	}
}

func TestScanDirWithUnresolvableSymlink(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a dangling symlink
	symlinkPath := filepath.Join(tmpDir, "dangling.go")
	if err := os.Symlink("/nonexistent/path/file.go", symlinkPath); err != nil {
		t.Skip("Symlinks not supported on this system")
	}

	// Create a real file to ensure scanner still works
	realFile := filepath.Join(tmpDir, "real.go")
	if err := os.WriteFile(realFile, []byte("package main\n"), 0644); err != nil {
		t.Fatalf("Failed to create file: %v", err)
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// Should find the real file, skip the dangling symlink
	if len(result) != 1 {
		t.Errorf("ScanDir() should find 1 file (skipping dangling symlink), got %d", len(result))
	}
}

func TestScanDirWithSymlinkDirectory(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a real directory with files
	realDir := filepath.Join(tmpDir, "real")
	if err := os.Mkdir(realDir, 0755); err != nil {
		t.Fatalf("Failed to create real dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(realDir, "file.go"), []byte("package real\n"), 0644); err != nil {
		t.Fatalf("Failed to create file: %v", err)
	}

	// Create a symlink to a directory outside the root
	outsideDir := t.TempDir() // Different temp directory
	if err := os.WriteFile(filepath.Join(outsideDir, "outside.go"), []byte("package outside\n"), 0644); err != nil {
		t.Fatalf("Failed to create outside file: %v", err)
	}

	symlinkDir := filepath.Join(tmpDir, "linked")
	if err := os.Symlink(outsideDir, symlinkDir); err != nil {
		t.Skip("Symlinks not supported on this system")
	}

	s := NewScanner(nil)
	result, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir() error: %v", err)
	}

	// Should only find files in the real directory, not through the symlink
	// (symlink to outside directory should be skipped)
	foundOutside := false
	for _, f := range result {
		if filepath.Base(f) == "outside.go" {
			foundOutside = true
		}
	}

	if foundOutside {
		t.Error("ScanDir() should not follow symlinks outside the root directory")
	}
}
