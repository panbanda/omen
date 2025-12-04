package commit

import (
	"runtime"
	"sync"

	"github.com/go-git/go-git/v5/plumbing"
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

	complexityAnalysis, err := a.complexity.AnalyzeProjectFromSource(files, src)
	if err != nil {
		return nil, err
	}

	return &CommitAnalysis{
		CommitHash: hash.String(),
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
	var firstErr error

	maxWorkers := runtime.NumCPU()
	p := pool.New().WithMaxGoroutines(maxWorkers)

	for i, hash := range hashes {
		i := i
		hash := hash
		p.Go(func() {
			// Each worker creates its own analyzer to avoid parser contention
			localAnalyzer := &Analyzer{complexity: complexity.New()}
			defer localAnalyzer.Close()

			result, err := localAnalyzer.AnalyzeCommit(repo, hash)
			if err != nil {
				mu.Lock()
				if firstErr == nil {
					firstErr = err
				}
				mu.Unlock()
				return
			}

			mu.Lock()
			results[i] = result
			mu.Unlock()
		})
	}
	p.Wait()

	if firstErr != nil {
		return nil, firstErr
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

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	a.complexity.Close()
}
