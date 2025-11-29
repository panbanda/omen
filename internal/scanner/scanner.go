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
func (s *Scanner) ScanDir(root string) ([]string, error) {
	files := make([]string, 0, 1024)

	s.loadGitignore(root)

	err := filepath.WalkDir(root, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return nil
		}

		relPath, _ := filepath.Rel(root, path)

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

	return files, err
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
