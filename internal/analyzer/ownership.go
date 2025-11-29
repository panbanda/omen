package analyzer

import (
	"sort"
	"strings"
	"time"

	"github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
)

// OwnershipAnalyzer calculates code ownership and bus factor.
type OwnershipAnalyzer struct {
	excludeTrivial bool
}

// OwnershipOption is a functional option for configuring OwnershipAnalyzer.
type OwnershipOption func(*OwnershipAnalyzer)

// WithOwnershipExcludeTrivial sets whether to exclude trivial lines from ownership analysis.
func WithOwnershipExcludeTrivial(exclude bool) OwnershipOption {
	return func(a *OwnershipAnalyzer) {
		a.excludeTrivial = exclude
	}
}

// NewOwnershipAnalyzer creates a new ownership analyzer.
func NewOwnershipAnalyzer(opts ...OwnershipOption) *OwnershipAnalyzer {
	a := &OwnershipAnalyzer{
		excludeTrivial: true, // Default: exclude trivial lines
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// repoHandle holds a git repository and its HEAD commit for reuse across files.
type repoHandle struct {
	repo   *git.Repository
	commit *object.Commit
}

// AnalyzeRepo analyzes ownership for all files in a repository.
func (a *OwnershipAnalyzer) AnalyzeRepo(repoPath string, files []string) (*models.OwnershipAnalysis, error) {
	return a.AnalyzeRepoWithProgress(repoPath, files, nil)
}

// AnalyzeRepoWithProgress analyzes ownership with progress callback.
func (a *OwnershipAnalyzer) AnalyzeRepoWithProgress(repoPath string, files []string, onProgress func()) (*models.OwnershipAnalysis, error) {
	// Validate repository exists and get HEAD hash upfront
	repo, err := git.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	head, err := repo.Head()
	if err != nil {
		return nil, err
	}
	headHash := head.Hash()

	// Process files in parallel using resource pool (one repo handle per worker)
	results := fileproc.ForEachFileWithResource(
		files,
		// initResource: create a repo handle per worker
		func() (*repoHandle, error) {
			r, err := git.PlainOpen(repoPath)
			if err != nil {
				return nil, err
			}
			c, err := r.CommitObject(headHash)
			if err != nil {
				return nil, err
			}
			return &repoHandle{repo: r, commit: c}, nil
		},
		// closeResource: nothing to close for git repos
		func(h *repoHandle) {},
		// fn: analyze file with pooled repo handle
		func(h *repoHandle, file string) (*models.FileOwnership, error) {
			return a.analyzeFile(h, repoPath, file)
		},
		onProgress,
	)

	// Collect non-nil results
	analysis := &models.OwnershipAnalysis{
		GeneratedAt: time.Now().UTC(),
		Files:       make([]models.FileOwnership, 0, len(results)),
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

// analyzeFile analyzes ownership for a single file using git blame.
func (a *OwnershipAnalyzer) analyzeFile(h *repoHandle, repoPath string, filePath string) (*models.FileOwnership, error) {
	// Get relative path for git blame
	relPath := filePath
	if strings.HasPrefix(filePath, repoPath) {
		relPath = strings.TrimPrefix(filePath, repoPath)
		relPath = strings.TrimPrefix(relPath, "/")
	}

	// Get blame using the pooled commit
	blame, err := git.Blame(h.commit, relPath)
	if err != nil {
		return nil, err
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
	var contributors []models.Contributor
	for _, ai := range authorLines {
		pct := float64(ai.lines) / float64(totalLines) * 100
		contributors = append(contributors, models.Contributor{
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

	concentration := models.CalculateConcentration(contributors)
	isSilo := len(contributors) == 1

	return &models.FileOwnership{
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
func (a *OwnershipAnalyzer) Close() {
	// No resources to release
}
