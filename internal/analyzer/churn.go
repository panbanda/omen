package analyzer

import (
	"context"
	"path/filepath"
	"sort"
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

	absPath, err := filepath.Abs(repoPath)
	if err != nil {
		absPath = repoPath
	}

	cutoff := time.Now().AddDate(0, 0, -a.days)

	logIter, err := repo.Log(&vcs.LogOptions{
		Since: &cutoff,
	})
	if err != nil {
		return nil, err
	}
	defer logIter.Close()

	fileMetrics := make(map[string]*models.FileChurnMetrics)

	err = logIter.ForEach(func(commit vcs.Commit) error {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if a.spinner != nil {
			a.spinner.Tick()
		}

		return a.processCommit(commit, fileMetrics)
	})

	if err != nil {
		return nil, err
	}

	return buildChurnAnalysis(fileMetrics, absPath, a.days), nil
}

// AnalyzeFiles analyzes churn for specific files.
func (a *ChurnAnalyzer) AnalyzeFiles(repoPath string, files []string) (*models.ChurnAnalysis, error) {
	fullAnalysis, err := a.AnalyzeRepo(repoPath)
	if err != nil {
		return nil, err
	}

	fileSet := make(map[string]bool)
	for _, f := range files {
		rel, err := filepath.Rel(repoPath, f)
		if err != nil {
			rel = f
		}
		fileSet[rel] = true
		fileSet[f] = true
		fileSet["./"+rel] = true
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

		sort.Slice(filtered.Files, func(i, j int) bool {
			return filtered.Files[i].ChurnScore > filtered.Files[j].ChurnScore
		})
		filtered.Summary.MaxChurnScore = filtered.Files[0].ChurnScore
	}

	return filtered, nil
}
