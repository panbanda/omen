package commit

import (
	"context"
	"runtime"
	"sync"
	"time"

	"github.com/go-git/go-git/v6/plumbing"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/panbanda/omen/pkg/source"
	"github.com/sourcegraph/conc/pool"
)

// Analyzer coordinates analysis at specific commits.
type Analyzer struct {
	complexity *complexity.Analyzer
}

// New creates a new commit analyzer.
func New() *Analyzer {
	return &Analyzer{
		complexity: complexity.New(),
	}
}

// AnalyzeCommit analyzes the repository state at a specific commit.
func (a *Analyzer) AnalyzeCommit(repo vcs.Repository, hash plumbing.Hash) (*CommitAnalysis, error) {
	commit, err := repo.CommitObject(hash)
	if err != nil {
		return nil, err
	}

	tree, err := commit.Tree()
	if err != nil {
		return nil, err
	}

	// Get all analyzable files from the tree
	entries, err := tree.Entries()
	if err != nil {
		return nil, err
	}

	var files []string
	for _, e := range entries {
		if !e.IsDir && parser.DetectLanguage(e.Path) != parser.LangUnknown {
			files = append(files, e.Path)
		}
	}

	src := source.NewTree(tree)

	complexityAnalysis, err := a.complexity.Analyze(context.Background(), files, src)
	if err != nil {
		return nil, err
	}

	return &CommitAnalysis{
		CommitHash: hash.String(),
		CommitDate: commit.Author().When,
		Complexity: complexityAnalysis,
	}, nil
}

// AnalyzeCommits analyzes multiple commits in parallel.
func (a *Analyzer) AnalyzeCommits(repo vcs.Repository, hashes []plumbing.Hash) ([]*CommitAnalysis, error) {
	if len(hashes) == 0 {
		return nil, nil
	}

	results := make([]*CommitAnalysis, len(hashes))
	var mu sync.Mutex

	maxWorkers := runtime.NumCPU() * 2 // Match DefaultWorkerMultiplier pattern
	p := pool.New().WithMaxGoroutines(maxWorkers).WithErrors()

	for i, hash := range hashes {
		p.Go(func() error {
			// Each worker creates its own analyzer to avoid parser contention
			localAnalyzer := &Analyzer{complexity: complexity.New()}
			defer localAnalyzer.Close()

			result, err := localAnalyzer.AnalyzeCommit(repo, hash)
			if err != nil {
				return err
			}

			mu.Lock()
			results[i] = result
			mu.Unlock()
			return nil
		})
	}

	if err := p.Wait(); err != nil {
		return nil, err
	}

	// Filter out nil results (shouldn't happen if no error)
	var filtered []*CommitAnalysis
	for _, r := range results {
		if r != nil {
			filtered = append(filtered, r)
		}
	}

	return filtered, nil
}

// AnalyzeTrend analyzes repository trends over a time period.
func (a *Analyzer) AnalyzeTrend(repo vcs.Repository, duration time.Duration) (*TrendAnalysis, error) {
	since := time.Now().Add(-duration)

	iter, err := repo.Log(&vcs.LogOptions{Since: &since})
	if err != nil {
		return nil, err
	}
	defer iter.Close()

	var hashes []plumbing.Hash
	var dates []time.Time

	err = iter.ForEach(func(c vcs.Commit) error {
		hashes = append(hashes, c.Hash())
		dates = append(dates, c.Author().When)
		return nil
	})
	if err != nil {
		return nil, err
	}

	if len(hashes) == 0 {
		return &TrendAnalysis{}, nil
	}

	// Analyze all commits in parallel
	results, err := a.AnalyzeCommits(repo, hashes)
	if err != nil {
		return nil, err
	}

	// Add dates to results
	for i, r := range results {
		if i < len(dates) {
			r.CommitDate = dates[i]
		}
	}

	return CalculateTrends(results), nil
}

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	a.complexity.Close()
}
