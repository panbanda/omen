package analyzer

import (
	"context"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/models"
)

// DefaultGitTimeout is the default timeout for git operations.
const DefaultGitTimeout = 5 * time.Minute

// ChurnAnalyzer analyzes git commit history for file churn.
type ChurnAnalyzer struct {
	days    int
	spinner *progress.Tracker
	opener  vcs.Opener
}

// ChurnOption is a functional option for configuring ChurnAnalyzer.
type ChurnOption func(*ChurnAnalyzer)

// WithChurnDays sets the number of days to analyze git history.
func WithChurnDays(days int) ChurnOption {
	return func(a *ChurnAnalyzer) {
		if days > 0 {
			a.days = days
		}
	}
}

// WithChurnSpinner sets a progress spinner for the analyzer.
func WithChurnSpinner(spinner *progress.Tracker) ChurnOption {
	return func(a *ChurnAnalyzer) {
		a.spinner = spinner
	}
}

// WithChurnOpener sets the VCS opener (useful for testing).
func WithChurnOpener(opener vcs.Opener) ChurnOption {
	return func(a *ChurnAnalyzer) {
		a.opener = opener
	}
}

// NewChurnAnalyzer creates a new churn analyzer.
func NewChurnAnalyzer(opts ...ChurnOption) *ChurnAnalyzer {
	a := &ChurnAnalyzer{
		days:    30,
		spinner: nil,
		opener:  vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// AnalyzeRepo analyzes git history for a repository.
func (a *ChurnAnalyzer) AnalyzeRepo(repoPath string) (*models.ChurnAnalysis, error) {
	ctx, cancel := context.WithTimeout(context.Background(), DefaultGitTimeout)
	defer cancel()
	return a.AnalyzeRepoWithContext(ctx, repoPath)
}

// AnalyzeRepoWithContext analyzes git history with a context for cancellation/timeout.
func (a *ChurnAnalyzer) AnalyzeRepoWithContext(ctx context.Context, repoPath string) (*models.ChurnAnalysis, error) {
	repo, err := a.opener.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	// Get absolute path for repository root
	absPath, err := filepath.Abs(repoPath)
	if err != nil {
		absPath = repoPath
	}

	// Calculate cutoff date
	cutoff := time.Now().AddDate(0, 0, -a.days)

	// Get commit log
	logIter, err := repo.Log(&vcs.LogOptions{
		Since: &cutoff,
	})
	if err != nil {
		return nil, err
	}
	defer logIter.Close()

	// Track file metrics - keyed by relative path
	fileMetrics := make(map[string]*models.FileChurnMetrics)

	err = logIter.ForEach(func(commit vcs.Commit) error {
		// Check for context cancellation
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}
		if a.spinner != nil {
			a.spinner.Tick()
		}

		// Get parent commit for diff
		if commit.NumParents() == 0 {
			return nil
		}

		parent, err := commit.Parent(0)
		if err != nil {
			return nil
		}

		// Get diff between commits
		parentTree, err := parent.Tree()
		if err != nil {
			return nil
		}

		commitTree, err := commit.Tree()
		if err != nil {
			return nil
		}

		changes, err := parentTree.Diff(commitTree)
		if err != nil {
			return nil
		}

		// Process each changed file
		for _, change := range changes {
			relativePath := change.ToName()
			if relativePath == "" {
				relativePath = change.FromName() // Deleted file
			}

			if _, exists := fileMetrics[relativePath]; !exists {
				fileMetrics[relativePath] = &models.FileChurnMetrics{
					Path:         "./" + relativePath, // pmat prefixes with ./
					RelativePath: relativePath,
					AuthorCounts: make(map[string]int),
					FirstCommit:  commit.Author().When,
					LastCommit:   commit.Author().When,
				}
			}

			fm := fileMetrics[relativePath]
			fm.Commits++
			// Use author name instead of email (matching pmat behavior)
			fm.AuthorCounts[commit.Author().Name]++

			// Track first and last commit times
			if commit.Author().When.Before(fm.FirstCommit) {
				fm.FirstCommit = commit.Author().When
			}
			if commit.Author().When.After(fm.LastCommit) {
				fm.LastCommit = commit.Author().When
			}

			// Count additions and deletions
			patch, err := change.Patch()
			if err == nil {
				for _, filePatch := range patch.FilePatches() {
					for _, chunk := range filePatch.Chunks() {
						content := chunk.Content()
						switch chunk.Type() {
						case vcs.ChunkAdd:
							fm.LinesAdded += countLines(content)
						case vcs.ChunkDelete:
							fm.LinesDeleted += countLines(content)
						}
					}
				}
			}
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	// Convert to slice and calculate scores
	analysis := &models.ChurnAnalysis{
		GeneratedAt:    time.Now().UTC(),
		PeriodDays:     a.days,
		RepositoryRoot: absPath,
		Files:          make([]models.FileChurnMetrics, 0, len(fileMetrics)),
		Summary:        models.NewChurnSummary(),
	}

	var totalCommits, totalAdded, totalDeleted int
	var maxCommits, maxChanges int

	// First pass: find max values for normalization
	for _, fm := range fileMetrics {
		if fm.Commits > maxCommits {
			maxCommits = fm.Commits
		}
		changes := fm.LinesAdded + fm.LinesDeleted
		if changes > maxChanges {
			maxChanges = changes
		}
	}

	// Second pass: calculate scores and collect stats
	for _, fm := range fileMetrics {
		// Convert author counts map to unique authors slice
		fm.UniqueAuthors = make([]string, 0, len(fm.AuthorCounts))
		for author := range fm.AuthorCounts {
			fm.UniqueAuthors = append(fm.UniqueAuthors, author)
		}

		fm.CalculateChurnScoreWithMax(maxCommits, maxChanges)
		analysis.Files = append(analysis.Files, *fm)

		totalCommits += fm.Commits
		totalAdded += fm.LinesAdded
		totalDeleted += fm.LinesDeleted

		// Aggregate author contributions
		for author, count := range fm.AuthorCounts {
			analysis.Summary.AuthorContributions[author] += count
		}
	}

	// Sort by churn score (highest first)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].ChurnScore > analysis.Files[j].ChurnScore
	})

	// Build summary
	analysis.Summary.TotalFilesChanged = len(analysis.Files)
	analysis.Summary.TotalCommits = totalCommits
	analysis.Summary.TotalAdditions = totalAdded
	analysis.Summary.TotalDeletions = totalDeleted

	if len(analysis.Files) > 0 {
		analysis.Summary.AvgCommitsPerFile = float64(totalCommits) / float64(len(analysis.Files))
		analysis.Summary.MaxChurnScore = analysis.Files[0].ChurnScore
	}

	// Calculate churn statistics
	analysis.Summary.CalculateStatistics(analysis.Files)

	// Identify hotspot and stable files
	analysis.Summary.IdentifyHotspotAndStableFiles(analysis.Files)

	return analysis, nil
}

// countLines counts the number of newlines in content.
func countLines(content string) int {
	return strings.Count(content, "\n")
}

// AnalyzeFiles analyzes churn for specific files.
func (a *ChurnAnalyzer) AnalyzeFiles(repoPath string, files []string) (*models.ChurnAnalysis, error) {
	fullAnalysis, err := a.AnalyzeRepo(repoPath)
	if err != nil {
		return nil, err
	}

	// Filter to only requested files
	fileSet := make(map[string]bool)
	for _, f := range files {
		// Normalize paths
		rel, err := filepath.Rel(repoPath, f)
		if err != nil {
			rel = f
		}
		fileSet[rel] = true
		fileSet[f] = true
		fileSet["./"+rel] = true // Also match pmat-style paths
	}

	filtered := &models.ChurnAnalysis{
		GeneratedAt:    time.Now().UTC(),
		PeriodDays:     a.days,
		RepositoryRoot: fullAnalysis.RepositoryRoot,
		Files:          make([]models.FileChurnMetrics, 0),
		Summary:        models.NewChurnSummary(),
	}

	for _, fm := range fullAnalysis.Files {
		if fileSet[fm.Path] || fileSet[fm.RelativePath] {
			filtered.Files = append(filtered.Files, fm)
		}
	}

	// Recalculate summary for filtered files
	var totalCommits, totalAdded, totalDeleted int

	for _, fm := range filtered.Files {
		totalCommits += fm.Commits
		totalAdded += fm.LinesAdded
		totalDeleted += fm.LinesDeleted
		for author, count := range fm.AuthorCounts {
			filtered.Summary.AuthorContributions[author] += count
		}
	}

	filtered.Summary.TotalFilesChanged = len(filtered.Files)
	filtered.Summary.TotalCommits = totalCommits
	filtered.Summary.TotalAdditions = totalAdded
	filtered.Summary.TotalDeletions = totalDeleted

	if len(filtered.Files) > 0 {
		filtered.Summary.AvgCommitsPerFile = float64(totalCommits) / float64(len(filtered.Files))

		// Sort and get max
		sort.Slice(filtered.Files, func(i, j int) bool {
			return filtered.Files[i].ChurnScore > filtered.Files[j].ChurnScore
		})
		filtered.Summary.MaxChurnScore = filtered.Files[0].ChurnScore
	}

	return filtered, nil
}
