package ownership

import (
	"bufio"
	"bytes"
	"errors"
	"os/exec"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/internal/vcs"
)

// ErrGitNotFound is returned when git is not available in PATH.
var ErrGitNotFound = errors.New("git executable not found in PATH (required for ownership analysis)")

// gitChecked tracks whether we've verified git availability.
var (
	gitCheckOnce sync.Once
	gitCheckErr  error
)

// checkGitAvailable verifies that git is installed and accessible.
func checkGitAvailable() error {
	gitCheckOnce.Do(func() {
		_, err := exec.LookPath("git")
		if err != nil {
			gitCheckErr = ErrGitNotFound
		}
	})
	return gitCheckErr
}

// blameLine represents a single line from git blame output.
type blameLine struct {
	Author     string
	AuthorName string
	Text       string
}

// blameResult contains blame information for a file.
type blameResult struct {
	Lines []blameLine
}

// runGitBlame executes native git blame and parses the output.
func runGitBlame(repoPath, relPath string) (*blameResult, error) {
	cmd := exec.Command("git", "blame", "--line-porcelain", "HEAD", "--", relPath)
	cmd.Dir = repoPath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	if err := cmd.Run(); err != nil {
		// Check for "no such path" or similar - return nil for non-existent files
		errStr := stderr.String()
		if strings.Contains(errStr, "no such path") ||
			strings.Contains(errStr, "does not exist") ||
			strings.Contains(errStr, "fatal: no such path") {
			return nil, nil
		}
		// Return nil for other errors (untracked files, binary files, etc.)
		if len(errStr) > 0 {
			return nil, nil
		}
		return nil, nil
	}

	return parseGitBlameOutput(&stdout), nil
}

// parseGitBlameOutput parses git blame --line-porcelain output.
func parseGitBlameOutput(r *bytes.Buffer) *blameResult {
	var lines []blameLine
	scanner := bufio.NewScanner(r)

	var currentLine blameLine
	for scanner.Scan() {
		line := scanner.Text()

		switch {
		case strings.HasPrefix(line, "author "):
			currentLine.AuthorName = strings.TrimPrefix(line, "author ")
		case strings.HasPrefix(line, "author-mail "):
			// Extract email from <email@example.com>
			email := strings.TrimPrefix(line, "author-mail ")
			email = strings.TrimPrefix(email, "<")
			email = strings.TrimSuffix(email, ">")
			currentLine.Author = email
		case strings.HasPrefix(line, "\t"):
			// Line content (prefixed with tab)
			currentLine.Text = strings.TrimPrefix(line, "\t")
			lines = append(lines, currentLine)
			currentLine = blameLine{}
		}
	}

	return &blameResult{Lines: lines}
}

// Analyzer calculates code ownership and bus factor.
type Analyzer struct {
	excludeTrivial bool
	opener         vcs.Opener
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithIncludeTrivial includes trivial lines in ownership analysis.
// By default, trivial lines (empty, braces-only, etc.) are excluded.
func WithIncludeTrivial() Option {
	return func(a *Analyzer) {
		a.excludeTrivial = false
	}
}

// WithOpener sets the VCS opener (useful for testing).
func WithOpener(opener vcs.Opener) Option {
	return func(a *Analyzer) {
		a.opener = opener
	}
}

// New creates a new ownership analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		excludeTrivial: true, // Default: exclude trivial lines
		opener:         vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// AnalyzeRepo analyzes ownership for all files in a repository.
func (a *Analyzer) AnalyzeRepo(repoPath string, files []string) (*Analysis, error) {
	return a.AnalyzeRepoWithProgress(repoPath, files, nil)
}

// maxGitWorkers limits concurrent git blame processes to avoid FD exhaustion.
// Git blame is I/O bound, so more workers don't help much beyond this point.
const maxGitWorkers = 8

// AnalyzeRepoWithProgress analyzes ownership with progress callback.
func (a *Analyzer) AnalyzeRepoWithProgress(repoPath string, files []string, onProgress func()) (*Analysis, error) {
	// Check git is available (required for native blame)
	if err := checkGitAvailable(); err != nil {
		return nil, err
	}

	// Validate repository exists using detect mode (works from subfolders)
	repo, err := a.opener.PlainOpenWithDetect(repoPath)
	if err != nil {
		return nil, err
	}

	// Get the actual repo root path - only open once, not per worker
	actualRepoPath := repo.RepoPath()

	// Process files in parallel with limited workers to avoid FD exhaustion
	results := fileproc.ForEachFileN(
		files,
		maxGitWorkers,
		func(file string) (*FileOwnership, error) {
			return a.analyzeFileNative(actualRepoPath, file)
		},
		onProgress,
		nil, // no error callback - silently skip failed files
	)

	// Collect non-nil results
	analysis := &Analysis{
		GeneratedAt: time.Now().UTC(),
		Files:       make([]FileOwnership, 0, len(results)),
	}

	for _, ownership := range results {
		if ownership != nil {
			analysis.Files = append(analysis.Files, *ownership)
		}
	}

	// Sort by concentration (highest first - most risky)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].Concentration > analysis.Files[j].Concentration
	})

	analysis.CalculateSummary()

	return analysis, nil
}

// analyzeFileNative analyzes ownership for a single file using native git blame.
func (a *Analyzer) analyzeFileNative(repoPath string, filePath string) (*FileOwnership, error) {
	// Get relative path for git blame
	relPath := filePath
	if strings.HasPrefix(filePath, repoPath) {
		relPath = strings.TrimPrefix(filePath, repoPath)
		relPath = strings.TrimPrefix(relPath, "/")
	}

	// Run native git blame directly
	blame, err := runGitBlame(repoPath, relPath)
	if err != nil {
		return nil, err
	}
	if blame == nil {
		return nil, nil // File not tracked
	}

	// Count lines per author
	authorLines := make(map[string]*authorInfo)
	var totalLines int

	for _, line := range blame.Lines {
		if a.excludeTrivial && isTrivialLine(line.Text) {
			continue
		}
		totalLines++

		author := line.AuthorName
		if author == "" {
			author = line.Author // Fallback to email if name is empty
		}
		if ai, ok := authorLines[author]; ok {
			ai.lines++
		} else {
			authorLines[author] = &authorInfo{
				name:  author,
				email: line.Author, // Author field contains email
				lines: 1,
			}
		}
	}

	if totalLines == 0 {
		return nil, nil // Skip empty or all-trivial files
	}

	// Build contributors list
	var contributors []Contributor
	for _, ai := range authorLines {
		pct := float64(ai.lines) / float64(totalLines) * 100
		contributors = append(contributors, Contributor{
			Name:       ai.name,
			Email:      ai.email,
			LinesOwned: ai.lines,
			Percentage: pct,
		})
	}

	// Sort by lines owned (descending)
	sort.Slice(contributors, func(i, j int) bool {
		return contributors[i].LinesOwned > contributors[j].LinesOwned
	})

	// Determine primary owner
	var primaryOwner string
	var ownershipPct float64
	if len(contributors) > 0 {
		primaryOwner = contributors[0].Name
		ownershipPct = contributors[0].Percentage
	}

	concentration := CalculateConcentration(contributors)
	isSilo := len(contributors) == 1

	return &FileOwnership{
		Path:             filePath,
		PrimaryOwner:     primaryOwner,
		OwnershipPercent: ownershipPct,
		Concentration:    concentration,
		TotalLines:       totalLines,
		Contributors:     contributors,
		IsSilo:           isSilo,
	}, nil
}

// authorInfo tracks author statistics.
type authorInfo struct {
	name  string
	email string
	lines int
}

// isTrivialLine checks if a line is trivial (blank, import, brace).
func isTrivialLine(text string) bool {
	trimmed := strings.TrimSpace(text)

	// Blank lines
	if trimmed == "" {
		return true
	}

	// Single braces/brackets
	if trimmed == "{" || trimmed == "}" || trimmed == "(" || trimmed == ")" ||
		trimmed == "[" || trimmed == "]" || trimmed == "});" || trimmed == "}," {
		return true
	}

	// Import statements (various languages)
	if strings.HasPrefix(trimmed, "import ") ||
		strings.HasPrefix(trimmed, "from ") ||
		strings.HasPrefix(trimmed, "#include ") ||
		strings.HasPrefix(trimmed, "use ") ||
		strings.HasPrefix(trimmed, "require ") ||
		strings.HasPrefix(trimmed, "using ") {
		return true
	}

	// Package declarations
	if strings.HasPrefix(trimmed, "package ") {
		return true
	}

	return false
}

// Close releases any resources.
func (a *Analyzer) Close() {
	// No resources to release
}
