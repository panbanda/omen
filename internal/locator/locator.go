package locator

import (
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/panbanda/omen/pkg/analyzer/repomap"
)

// TargetType indicates whether the focus resolved to a file or symbol.
type TargetType string

const (
	TargetFile   TargetType = "file"
	TargetSymbol TargetType = "symbol"
)

// Symbol represents a resolved code symbol.
type Symbol struct {
	Name string
	Kind string
	File string
	Line int
}

// Candidate represents an ambiguous match option.
type Candidate struct {
	Path string
	Name string
	File string
	Line int
	Kind string
}

// Result contains the resolved target or error information.
type Result struct {
	Type       TargetType
	Path       string
	Symbol     *Symbol
	Candidates []Candidate
}

var (
	ErrNotFound       = errors.New("no file or symbol found")
	ErrAmbiguousMatch = errors.New("ambiguous match")
	ErrTooManyMatches = errors.New("too many matches")
)

// Options configures the Locate behavior.
type Options struct {
	BaseDir string
}

// Option is a functional option for Locate.
type Option func(*Options)

// WithBaseDir sets the base directory for glob and basename searches.
func WithBaseDir(dir string) Option {
	return func(o *Options) {
		o.BaseDir = dir
	}
}

// Locate resolves a focus target to a file or symbol.
// Resolution order: exact path -> glob -> basename -> symbol
func Locate(focus string, files []string, repoMap *repomap.Map, opts ...Option) (*Result, error) {
	options := &Options{
		BaseDir: ".",
	}
	for _, opt := range opts {
		opt(options)
	}

	// Try exact file path first
	if info, err := os.Stat(focus); err == nil && !info.IsDir() {
		return &Result{
			Type: TargetFile,
			Path: focus,
		}, nil
	}

	// Try glob pattern if contains glob characters
	if containsGlobChars(focus) {
		return locateByGlob(focus, options.BaseDir)
	}

	// Try basename search if looks like a filename (has extension)
	if looksLikeFilename(focus) {
		return locateByBasename(focus, options.BaseDir)
	}

	// Try symbol search if repo map is available
	if repoMap != nil {
		return locateBySymbol(focus, repoMap)
	}

	// No match found
	return nil, ErrNotFound
}

func containsGlobChars(s string) bool {
	return strings.Contains(s, "*") || strings.Contains(s, "?") || strings.Contains(s, "[")
}

func locateByGlob(pattern, baseDir string) (*Result, error) {
	matches, err := doublestar.Glob(os.DirFS(baseDir), pattern)
	if err != nil {
		return nil, err
	}

	if len(matches) == 0 {
		return nil, ErrNotFound
	}

	// Convert to absolute paths
	var absPaths []string
	for _, m := range matches {
		absPaths = append(absPaths, filepath.Join(baseDir, m))
	}

	if len(absPaths) == 1 {
		return &Result{
			Type: TargetFile,
			Path: absPaths[0],
		}, nil
	}

	// Multiple matches - return candidates
	candidates := make([]Candidate, len(absPaths))
	for i, p := range absPaths {
		candidates[i] = Candidate{Path: p}
	}
	return &Result{Candidates: candidates}, ErrAmbiguousMatch
}

func looksLikeFilename(s string) bool {
	ext := filepath.Ext(s)
	return ext != "" && !strings.Contains(s, string(filepath.Separator))
}

func locateByBasename(filename, baseDir string) (*Result, error) {
	// Use glob to find all files with this basename
	pattern := "**/" + filename
	return locateByGlob(pattern, baseDir)
}

func locateBySymbol(name string, repoMap *repomap.Map) (*Result, error) {
	var matches []repomap.Symbol
	for _, sym := range repoMap.Symbols {
		if sym.Name == name {
			matches = append(matches, sym)
		}
	}

	if len(matches) == 0 {
		return nil, ErrNotFound
	}

	if len(matches) == 1 {
		return &Result{
			Type: TargetSymbol,
			Symbol: &Symbol{
				Name: matches[0].Name,
				Kind: matches[0].Kind,
				File: matches[0].File,
				Line: matches[0].Line,
			},
		}, nil
	}

	// Multiple matches - return candidates
	candidates := make([]Candidate, len(matches))
	for i, m := range matches {
		candidates[i] = Candidate{
			Name: m.Name,
			File: m.File,
			Line: m.Line,
			Kind: m.Kind,
		}
	}
	return &Result{Candidates: candidates}, ErrAmbiguousMatch
}
