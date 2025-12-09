package scanner

import (
	"path/filepath"

	"github.com/panbanda/omen/internal/scanner"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/parser"
)

// ScanResult contains the result of a file scan.
type ScanResult struct {
	Files          []string
	LanguageGroups map[parser.Language][]string
	RepoRoot       string
}

// Service provides file scanning functionality.
type Service struct {
	config *config.Config
	opener vcs.Opener
}

// Option configures a Service.
type Option func(*Service)

// WithConfig sets the configuration.
func WithConfig(cfg *config.Config) Option {
	return func(s *Service) {
		s.config = cfg
	}
}

// WithOpener sets the VCS opener (for testing).
func WithOpener(opener vcs.Opener) Option {
	return func(s *Service) {
		s.opener = opener
	}
}

// New creates a new scanner service.
func New(opts ...Option) *Service {
	cfg, _ := config.LoadOrDefault()
	if cfg == nil {
		cfg = config.DefaultConfig()
	}
	s := &Service{
		config: cfg,
		opener: vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(s)
	}
	return s
}

// ScanPaths scans multiple paths and returns all found source files.
func (s *Service) ScanPaths(paths []string) (*ScanResult, error) {
	if len(paths) == 0 {
		paths = []string{"."}
	}

	scan := scanner.NewScanner(s.config)
	var files []string

	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return nil, &PathError{Path: path, Err: err}
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return nil, &ScanError{Path: path, Err: err}
		}
		files = append(files, found...)
	}

	result := &ScanResult{
		Files:          files,
		LanguageGroups: scan.GroupByLanguage(files),
	}

	return result, nil
}

// ScanPathsForGit scans paths and also resolves the git repository root.
// Returns an error if not in a git repository when gitRequired is true.
func (s *Service) ScanPathsForGit(paths []string, gitRequired bool) (*ScanResult, error) {
	result, err := s.ScanPaths(paths)
	if err != nil {
		return nil, err
	}

	if len(paths) == 0 {
		paths = []string{"."}
	}

	repoRoot, err := s.findGitRoot(paths[0])
	if err != nil {
		if gitRequired {
			return nil, &GitError{Err: err}
		}
		// Not required, continue without repo root
	} else {
		result.RepoRoot = repoRoot
	}

	return result, nil
}

// findGitRoot finds the git repository root containing the given path.
func (s *Service) findGitRoot(path string) (string, error) {
	absPath, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}

	// Try to open git repo starting from path
	repo, err := s.opener.PlainOpenWithDetect(absPath)
	if err != nil {
		return "", err
	}

	return repo.RepoPath(), nil
}

// FilterBySize filters files by maximum size.
func (s *Service) FilterBySize(files []string, maxSize int64) ([]string, int) {
	return scanner.FilterBySize(files, maxSize)
}

// PathError indicates an invalid path.
type PathError struct {
	Path string
	Err  error
}

func (e *PathError) Error() string {
	return "invalid path " + e.Path + ": " + e.Err.Error()
}

func (e *PathError) Unwrap() error {
	return e.Err
}

// ScanError indicates a scanning failure.
type ScanError struct {
	Path string
	Err  error
}

func (e *ScanError) Error() string {
	return "failed to scan directory " + e.Path + ": " + e.Err.Error()
}

func (e *ScanError) Unwrap() error {
	return e.Err
}

// GitError indicates the path is not a git repository.
type GitError struct {
	Err error
}

func (e *GitError) Error() string {
	return "not a git repository (or any parent): " + e.Err.Error()
}

func (e *GitError) Unwrap() error {
	return e.Err
}
