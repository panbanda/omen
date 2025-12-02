package temporal

import (
	"context"
	"sort"
	"time"

	"github.com/panbanda/omen/internal/vcs"
)

// DefaultGitTimeout is the default timeout for git operations.
const DefaultGitTimeout = 5 * time.Minute

// Analyzer identifies files that frequently change together.
type Analyzer struct {
	days         int
	minCochanges int
	opener       vcs.Opener
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithOpener sets the VCS opener (useful for testing).
func WithOpener(opener vcs.Opener) Option {
	return func(a *Analyzer) {
		a.opener = opener
	}
}

// New creates a new temporal coupling analyzer.
func New(days, minCochanges int, opts ...Option) *Analyzer {
	if days <= 0 {
		days = 30
	}
	if minCochanges <= 0 {
		minCochanges = DefaultMinCochanges
	}
	a := &Analyzer{
		days:         days,
		minCochanges: minCochanges,
		opener:       vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// filePair represents an unordered pair of files.
type filePair struct {
	a, b string
}

// makeFilePair creates a normalized file pair (alphabetically ordered).
func makeFilePair(a, b string) filePair {
	if a > b {
		a, b = b, a
	}
	return filePair{a: a, b: b}
}

// AnalyzeRepo analyzes temporal coupling for a repository.
func (a *Analyzer) AnalyzeRepo(repoPath string) (*Analysis, error) {
	ctx, cancel := context.WithTimeout(context.Background(), DefaultGitTimeout)
	defer cancel()
	return a.AnalyzeRepoWithContext(ctx, repoPath)
}

// AnalyzeRepoWithContext analyzes temporal coupling with a context for cancellation/timeout.
func (a *Analyzer) AnalyzeRepoWithContext(ctx context.Context, repoPath string) (*Analysis, error) {
	repo, err := a.opener.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	since := time.Now().AddDate(0, 0, -a.days)

	// Get commit history
	logIter, err := repo.Log(&vcs.LogOptions{
		Since: &since,
	})
	if err != nil {
		return nil, err
	}

	// Track co-changes: filePair -> count
	cochanges := make(map[filePair]int)
	// Track individual file commits: file -> count
	fileCommits := make(map[string]int)

	err = logIter.ForEach(func(c vcs.Commit) error {
		// Check for context cancellation
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}
		stats, err := c.Stats()
		if err != nil {
			return nil // Skip commits we can't get stats for
		}

		// Collect files changed in this commit
		var changedFiles []string
		for _, stat := range stats {
			changedFiles = append(changedFiles, stat.Name)
			fileCommits[stat.Name]++
		}

		// Record co-changes for all pairs
		for i := 0; i < len(changedFiles); i++ {
			for j := i + 1; j < len(changedFiles); j++ {
				pair := makeFilePair(changedFiles[i], changedFiles[j])
				cochanges[pair]++
			}
		}

		return nil
	})
	if err != nil {
		return nil, err
	}

	// Build coupling results, filtering by minimum threshold
	var couplings []FileCoupling
	for pair, count := range cochanges {
		if count >= a.minCochanges {
			commitsA := fileCommits[pair.a]
			commitsB := fileCommits[pair.b]
			strength := CalculateCouplingStrength(count, commitsA, commitsB)

			couplings = append(couplings, FileCoupling{
				FileA:            pair.a,
				FileB:            pair.b,
				CochangeCount:    count,
				CouplingStrength: strength,
				CommitsA:         commitsA,
				CommitsB:         commitsB,
			})
		}
	}

	// Sort by coupling strength (highest first)
	sort.Slice(couplings, func(i, j int) bool {
		return couplings[i].CouplingStrength > couplings[j].CouplingStrength
	})

	analysis := &Analysis{
		GeneratedAt:  time.Now().UTC(),
		PeriodDays:   a.days,
		MinCochanges: a.minCochanges,
		Couplings:    couplings,
	}
	analysis.CalculateSummary(len(fileCommits))

	return analysis, nil
}

// Close releases any resources.
func (a *Analyzer) Close() {
	// No resources to release
}
