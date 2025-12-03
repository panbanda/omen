package score

import (
	"context"
	"time"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/analyzer/cohesion"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/defect"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/sourcegraph/conc"
)

// Analyzer orchestrates component analyzers to produce a composite score.
type Analyzer struct {
	weights     Weights
	thresholds  Thresholds
	churnDays   int
	maxFileSize int64
}

// Option configures the Analyzer.
type Option func(*Analyzer)

// WithWeights sets custom weights for the composite score.
func WithWeights(w Weights) Option {
	return func(a *Analyzer) {
		a.weights = w
	}
}

// WithThresholds sets minimum score thresholds.
func WithThresholds(t Thresholds) Option {
	return func(a *Analyzer) {
		a.thresholds = t
	}
}

// WithChurnDays sets the git history period for churn analysis.
func WithChurnDays(days int) Option {
	return func(a *Analyzer) {
		a.churnDays = days
	}
}

// WithMaxFileSize sets the maximum file size to analyze.
func WithMaxFileSize(size int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = size
	}
}

// New creates a new score analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		weights:    DefaultWeights(),
		thresholds: Thresholds{},
		churnDays:  30,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// ProgressFunc is called to report analysis progress.
type ProgressFunc func(stage string)

// AnalyzeProject computes the repository score for the given files.
func (a *Analyzer) AnalyzeProject(ctx context.Context, repoPath string, files []string) (*Result, error) {
	return a.AnalyzeProjectWithProgress(ctx, repoPath, files, nil)
}

// AnalyzeProjectWithProgress computes the score with progress reporting.
func (a *Analyzer) AnalyzeProjectWithProgress(ctx context.Context, repoPath string, files []string, onProgress ProgressFunc) (*Result, error) {
	// Create sub-analyzers
	complexityAnalyzer := complexity.New(complexity.WithMaxFileSize(a.maxFileSize))
	defer complexityAnalyzer.Close()

	duplicatesAnalyzer := duplicates.New(
		duplicates.WithMinTokens(6),
		duplicates.WithSimilarityThreshold(0.8),
		duplicates.WithMaxFileSize(a.maxFileSize),
	)
	defer duplicatesAnalyzer.Close()

	defectAnalyzer := defect.New(
		defect.WithChurnDays(a.churnDays),
		defect.WithMaxFileSize(a.maxFileSize),
	)
	defer defectAnalyzer.Close()

	satdAnalyzer := satd.New()

	graphAnalyzer := graph.New(graph.WithScope(graph.ScopeModule), graph.WithMaxFileSize(a.maxFileSize))
	defer graphAnalyzer.Close()

	smellsAnalyzer := smells.New()

	cohesionAnalyzer := cohesion.New(cohesion.WithMaxFileSize(a.maxFileSize))
	defer cohesionAnalyzer.Close()

	result := &Result{
		Weights:   a.weights,
		Timestamp: time.Now().UTC(),
		Passed:    true,
	}

	// Get current commit
	opener := vcs.NewGitOpener()
	if repo, err := opener.PlainOpenWithDetect(repoPath); err == nil {
		if head, err := repo.Head(); err == nil {
			hash := head.Hash().String()
			if len(hash) >= 7 {
				result.Commit = hash[:7]
			}
		}
	}

	// Run analyzers in parallel
	var (
		cxResult       *complexity.Analysis
		dupResult      *duplicates.Analysis
		defectResult   *defect.Analysis
		satdResult     *satd.Analysis
		graphResult    *graph.DependencyGraph
		cohesionResult *cohesion.Analysis
	)

	wg := conc.NewWaitGroup()

	wg.Go(func() {
		if onProgress != nil {
			onProgress("complexity")
		}
		cxResult, _ = complexityAnalyzer.AnalyzeProject(files)
	})
	wg.Go(func() {
		if onProgress != nil {
			onProgress("duplicates")
		}
		dupResult, _ = duplicatesAnalyzer.AnalyzeProject(files)
	})
	wg.Go(func() {
		if onProgress != nil {
			onProgress("defect")
		}
		defectResult, _ = defectAnalyzer.AnalyzeProject(ctx, repoPath, files)
	})
	wg.Go(func() {
		if onProgress != nil {
			onProgress("satd")
		}
		satdResult, _ = satdAnalyzer.AnalyzeProject(files)
	})
	wg.Go(func() {
		if onProgress != nil {
			onProgress("graph")
		}
		graphResult, _ = graphAnalyzer.AnalyzeProject(files)
	})
	wg.Go(func() {
		if onProgress != nil {
			onProgress("cohesion")
		}
		cohesionResult, _ = cohesionAnalyzer.AnalyzeProject(files)
	})

	wg.Wait()

	// Analyze smells from graph (must be after graph completes)
	var smellResult *smells.Analysis
	if graphResult != nil {
		if onProgress != nil {
			onProgress("smells")
		}
		smellResult = smellsAnalyzer.AnalyzeGraph(graphResult)
	}

	// Normalize each component
	result.FilesAnalyzed = len(files)

	// Complexity: 100 - (% functions exceeding threshold)
	if cxResult != nil {
		violating := 0
		total := cxResult.Summary.TotalFunctions
		for _, f := range cxResult.Files {
			for _, fn := range f.Functions {
				if fn.Metrics.Cyclomatic > 10 || fn.Metrics.Cognitive > 15 {
					violating++
				}
			}
		}
		result.Components.Complexity = NormalizeComplexity(total, violating)
	} else {
		result.Components.Complexity = 100
	}

	// Duplication: 100 - (ratio * 100)
	if dupResult != nil {
		result.Components.Duplication = NormalizeDuplication(dupResult.Summary.DuplicationRatio)
	} else {
		result.Components.Duplication = 100
	}

	// Defect: 100 - (avg probability * 100)
	if defectResult != nil {
		result.Components.Defect = NormalizeDefect(defectResult.Summary.AvgProbability)
	} else {
		result.Components.Defect = 100
	}

	// Debt: severity-weighted density per 1K LOC
	if satdResult != nil {
		loc := estimateLOC(files)
		counts := DebtSeverityCounts{
			Critical: satdResult.Summary.BySeverity["critical"],
			High:     satdResult.Summary.BySeverity["high"],
			Medium:   satdResult.Summary.BySeverity["medium"],
			Low:      satdResult.Summary.BySeverity["low"],
		}
		result.Components.Debt = NormalizeDebt(counts, loc)
	} else {
		result.Components.Debt = 100
	}

	// Coupling: combines cycles, SDP violations, and instability
	if smellResult != nil {
		metrics := CouplingMetrics{
			CyclicCount:        smellResult.Summary.CyclicCount,
			SDPViolations:      smellResult.Summary.UnstableCount,
			AverageInstability: smellResult.Summary.AverageInstability,
			TotalComponents:    smellResult.Summary.TotalComponents,
		}
		result.Components.Coupling = NormalizeCoupling(metrics)
	} else {
		result.Components.Coupling = 75 // Neutral when no data
	}

	// Smells: scaled by codebase size
	if smellResult != nil {
		counts := SmellCounts{
			Critical: smellResult.Summary.CyclicCount,
			High:     smellResult.Summary.GodCount + smellResult.Summary.HubCount,
			Medium:   smellResult.Summary.UnstableCount,
		}
		result.Components.Smells = NormalizeSmells(counts, smellResult.Summary.TotalComponents)
	} else {
		result.Components.Smells = 100
	}

	// Cohesion: reported separately
	if cohesionResult != nil && cohesionResult.Summary.TotalClasses > 0 {
		result.Cohesion = NormalizeCohesion(cohesionResult.Summary.AvgLCOM)
	} else {
		result.Cohesion = 100
	}

	// Compute composite and check thresholds
	result.ComputeComposite()
	result.CheckThresholds(a.thresholds)

	return result, nil
}

// estimateLOC estimates lines of code from file count.
func estimateLOC(files []string) int {
	// Rough estimate: average of 100 LOC per file
	return len(files) * 100
}
