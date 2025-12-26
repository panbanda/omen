package score

import (
	"context"
	"time"

	"github.com/panbanda/omen/pkg/analyzer/cohesion"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/panbanda/omen/pkg/analyzer/tdg"
	"github.com/panbanda/omen/pkg/source"
	"github.com/sourcegraph/conc"
)

// ContentSource is an alias for source.ContentSource for backward compatibility.
// New code should import from pkg/source directly.
type ContentSource = source.ContentSource

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

// Analyze computes the score from a ContentSource (e.g., git tree or filesystem).
// This enables analysis of historical commits without filesystem checkouts.
func (a *Analyzer) Analyze(ctx context.Context, files []string, src ContentSource, commitHash string) (*Result, error) {
	// Create sub-analyzers
	complexityAnalyzer := complexity.New(complexity.WithMaxFileSize(a.maxFileSize))
	defer complexityAnalyzer.Close()

	duplicatesAnalyzer := duplicates.New(
		duplicates.WithMinTokens(6),
		duplicates.WithSimilarityThreshold(0.8),
		duplicates.WithMaxFileSize(a.maxFileSize),
	)
	defer duplicatesAnalyzer.Close()

	satdAnalyzer := satd.New()
	defer satdAnalyzer.Close()

	tdgAnalyzer := tdg.New(tdg.WithMaxFileSize(a.maxFileSize))
	defer tdgAnalyzer.Close()

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

	// Set commit hash if provided
	if len(commitHash) >= 7 {
		result.Commit = commitHash[:7]
	}

	// Run analyzers in parallel using source-based methods
	var (
		cxResult       *complexity.Analysis
		dupResult      *duplicates.Analysis
		satdResult     *satd.Analysis
		tdgResult      *tdg.Analysis
		graphResult    *graph.DependencyGraph
		cohesionResult *cohesion.Analysis
	)

	wg := conc.NewWaitGroup()

	wg.Go(func() {
		cxResult, _ = complexityAnalyzer.Analyze(ctx, files, src)
	})
	wg.Go(func() {
		dupResult, _ = duplicatesAnalyzer.Analyze(ctx, files, src)
	})
	wg.Go(func() {
		satdResult, _ = satdAnalyzer.Analyze(ctx, files, src)
	})
	wg.Go(func() {
		tdgResult, _ = tdgAnalyzer.Analyze(ctx, files, src)
	})
	wg.Go(func() {
		graphResult, _ = graphAnalyzer.Analyze(ctx, files, src)
	})
	wg.Go(func() {
		cohesionResult, _ = cohesionAnalyzer.Analyze(ctx, files, src)
	})

	wg.Wait()

	// Analyze smells from graph (must be after graph completes)
	var smellResult *smells.Analysis
	if graphResult != nil {
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

	// SATD: severity-weighted density per 1K LOC
	if satdResult != nil {
		loc := estimateLOC(files)
		counts := SATDSeverityCounts{
			Critical: satdResult.Summary.BySeverity["critical"],
			High:     satdResult.Summary.BySeverity["high"],
			Medium:   satdResult.Summary.BySeverity["medium"],
			Low:      satdResult.Summary.BySeverity["low"],
		}
		result.Components.SATD = NormalizeSATD(counts, loc)
	} else {
		result.Components.SATD = 100
	}

	// TDG: Technical Debt Gradient comprehensive score
	if tdgResult != nil {
		result.Components.TDG = NormalizeTDG(tdgResult.AverageScore)
	} else {
		result.Components.TDG = 100
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

	// Cohesion: included in composite when weight > 0
	if cohesionResult != nil && cohesionResult.Summary.TotalClasses > 0 {
		result.Components.Cohesion = NormalizeCohesion(cohesionResult.Summary.AvgLCOM)
	} else {
		result.Components.Cohesion = 100
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
