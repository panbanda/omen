package scanner

import (
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/go-git/go-git/v5/plumbing/format/gitignore"
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

// loadGitignore loads .gitignore patterns from a directory up to the git root.
func (s *Scanner) loadGitignore(root string) {
	if !s.config.Exclude.Gitignore {
		return
	}

	var patterns []gitignore.Pattern

	// Find git root to limit how far up we walk
	gitRoot := findGitRoot(root)

	// Walk up to find all .gitignore files, stopping at git root
	dir := root
	for {
		gitignorePath := filepath.Join(dir, ".gitignore")
		if data, err := os.ReadFile(gitignorePath); err == nil {
			domain := strings.Split(strings.TrimPrefix(dir, root), string(filepath.Separator))
			for _, line := range strings.Split(string(data), "\n") {
				line = strings.TrimSpace(line)
				if line == "" || strings.HasPrefix(line, "#") {
					continue
				}
				patterns = append(patterns, gitignore.ParsePattern(line, domain))
			}
		}

		// Stop at git root or filesystem root
		if gitRoot != "" && dir == gitRoot {
			break
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}

	if len(patterns) > 0 {
		s.matchers = append(s.matchers, gitignore.NewMatcher(patterns))
	}
}

// isGitignored checks if a path matches gitignore patterns.
func (s *Scanner) isGitignored(path string, isDir bool) bool {
	if !s.config.Exclude.Gitignore || len(s.matchers) == 0 {
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

	s.loadGitignore(root)

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
			for _, excluded := range s.config.Exclude.Dirs {
				if d.Name() == excluded {
					return filepath.SkipDir
				}
			}
			if s.isGitignored(relPath, true) {
				return filepath.SkipDir
			}
			return nil
		}

		if s.isGitignored(relPath, false) {
			return nil
		}
		if s.config.ShouldExclude(path) {
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

	if s.config.ShouldExclude(path) {
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
