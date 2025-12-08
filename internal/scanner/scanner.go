package scanner

import (
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/go-git/go-billy/v6/osfs"
	"github.com/go-git/go-git/v6/plumbing/format/gitignore"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/parser"
)

// Scanner finds source files in a directory.
type Scanner struct {
	config   *config.Config
	matchers []gitignore.Matcher
}

// NewScanner creates a new file scanner.
func NewScanner(cfg *config.Config) *Scanner {
	if cfg == nil {
		cfg = config.DefaultConfig()
	}
	return &Scanner{config: cfg}
}

// findGitRoot finds the root of the git repository by looking for .git directory.
// Returns empty string if not in a git repository.
func findGitRoot(start string) string {
	dir := start
	for {
		gitDir := filepath.Join(dir, ".git")
		if info, err := os.Stat(gitDir); err == nil && info.IsDir() {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return ""
		}
		dir = parent
	}
}

// loadExcludePatterns loads exclusion patterns from both config and .gitignore files.
// Config patterns are parsed as gitignore patterns and combined with .gitignore files.
func (s *Scanner) loadExcludePatterns(root string) {
	var patterns []gitignore.Pattern

	// Add config exclude patterns (always applied, parsed as gitignore syntax)
	for _, pattern := range s.config.Exclude.Patterns {
		patterns = append(patterns, gitignore.ParsePattern(pattern, nil))
	}

	// Add .gitignore patterns if enabled using ReadPatterns which recursively
	// reads all .gitignore files in the directory tree
	if s.config.Exclude.Gitignore {
		gitRoot := findGitRoot(root)
		if gitRoot != "" {
			// Use osfs rooted at git root to read all .gitignore files
			fs := osfs.New(gitRoot)
			if gitPatterns, err := gitignore.ReadPatterns(fs, nil); err == nil {
				patterns = append(patterns, gitPatterns...)
			}
		}
	}

	if len(patterns) > 0 {
		s.matchers = append(s.matchers, gitignore.NewMatcher(patterns))
	}
}

// isExcluded checks if a path matches any exclusion pattern.
func (s *Scanner) isExcluded(path string, isDir bool) bool {
	if len(s.matchers) == 0 {
		return false
	}

	pathParts := strings.Split(path, string(filepath.Separator))
	for _, m := range s.matchers {
		if m.Match(pathParts, isDir) {
			return true
		}
	}
	return false
}

// ScanDir recursively scans a directory for source files.
// Uses filepath.WalkDir for better performance (avoids stat calls).
// Validates that all paths stay within the root directory to prevent traversal attacks.
func (s *Scanner) ScanDir(root string) ([]string, error) {
	files := make([]string, 0, 1024)

	// Resolve root to absolute path for security validation
	absRoot, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	// Resolve any symlinks in the root path
	absRoot, err = filepath.EvalSymlinks(absRoot)
	if err != nil {
		return nil, err
	}

	s.loadExcludePatterns(root)

	walkErr := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil
		}

		relPath, _ := filepath.Rel(root, path)

		// Security: validate path stays within root (prevent symlink traversal)
		if d.Type()&fs.ModeSymlink != 0 {
			// Resolve the symlink and check if it escapes root
			resolved, err := filepath.EvalSymlinks(path)
			if err != nil {
				// Skip unresolvable symlinks
				if d.IsDir() {
					return filepath.SkipDir
				}
				return nil
			}
			if !isWithinRoot(resolved, absRoot) {
				// Symlink points outside root - skip it
				if d.IsDir() {
					return filepath.SkipDir
				}
				return nil
			}
		}

		if d.IsDir() {
			if s.isExcluded(relPath, true) {
				return filepath.SkipDir
			}
			return nil
		}

		if s.isExcluded(relPath, false) {
			return nil
		}
		if parser.DetectLanguage(path) != parser.LangUnknown {
			files = append(files, path)
		}

		return nil
	})

	return files, walkErr
}

// isWithinRoot checks if a path is contained within the root directory.
// Returns false if the path escapes via symlinks or relative paths.
func isWithinRoot(path, root string) bool {
	// Ensure both are absolute
	absPath, err := filepath.Abs(path)
	if err != nil {
		return false
	}

	// Clean the paths to normalize them
	absPath = filepath.Clean(absPath)
	root = filepath.Clean(root)

	// Check if path starts with root
	// Add separator to prevent "/root2" matching "/root"
	if !strings.HasPrefix(absPath, root+string(filepath.Separator)) && absPath != root {
		return false
	}

	return true
}

// ScanFile checks if a single file should be analyzed.
func (s *Scanner) ScanFile(path string) (bool, error) {
	info, err := os.Stat(path)
	if err != nil {
		return false, err
	}

	if info.IsDir() {
		return false, nil
	}

	// Load exclude patterns if not already loaded
	if len(s.matchers) == 0 {
		dir := filepath.Dir(path)
		s.loadExcludePatterns(dir)
	}

	if s.isExcluded(filepath.Base(path), false) {
		return false, nil
	}

	return parser.DetectLanguage(path) != parser.LangUnknown, nil
}

// FilterByLanguage filters files to only those of a specific language.
func (s *Scanner) FilterByLanguage(files []string, lang parser.Language) []string {
	var filtered []string
	for _, f := range files {
		if parser.DetectLanguage(f) == lang {
			filtered = append(filtered, f)
		}
	}
	return filtered
}

// GroupByLanguage groups files by their detected language.
func (s *Scanner) GroupByLanguage(files []string) map[parser.Language][]string {
	groups := make(map[parser.Language][]string)
	for _, f := range files {
		lang := parser.DetectLanguage(f)
		if lang != parser.LangUnknown {
			groups[lang] = append(groups[lang], f)
		}
	}
	return groups
}

// FilterBySize filters files that exceed the configured maximum size.
// Returns the filtered list and the count of files that were skipped.
// If maxSize is 0, returns the original list unchanged.
func FilterBySize(files []string, maxSize int64) ([]string, int) {
	if maxSize <= 0 {
		return files, 0
	}

	filtered := make([]string, 0, len(files))
	skipped := 0

	for _, f := range files {
		info, err := os.Stat(f)
		if err != nil {
			skipped++
			continue
		}
		if info.Size() > maxSize {
			skipped++
			continue
		}
		filtered = append(filtered, f)
	}

	return filtered, skipped
}
