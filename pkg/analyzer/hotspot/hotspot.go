package hotspot

import (
	"context"
	"path/filepath"
	"sort"
	"time"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/analyzer/churn"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/parser"
)

// Analyzer identifies code hotspots using the geometric mean of
// CDF-normalized churn and complexity scores. This approach:
//   - Uses empirical CDFs to normalize metrics against industry benchmarks
//   - Applies geometric mean to require BOTH factors to be elevated
//   - Produces scores where 0.5+ indicates meaningful hotspots
type Analyzer struct {
	churnDays   int
	maxFileSize int64
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithChurnDays sets the number of days of git history to analyze.
func WithChurnDays(days int) Option {
	return func(a *Analyzer) {
		a.churnDays = days
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// New creates a new hotspot analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		churnDays:   30,
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// AnalyzeProject analyzes hotspots for a project.
func (a *Analyzer) AnalyzeProject(ctx context.Context, repoPath string, files []string) (*Analysis, error) {
	return a.AnalyzeProjectWithProgress(ctx, repoPath, files, nil)
}

// AnalyzeProjectWithProgress analyzes hotspots with optional progress callback.
func (a *Analyzer) AnalyzeProjectWithProgress(ctx context.Context, repoPath string, files []string, onProgress fileproc.ProgressFunc) (*Analysis, error) {
	// Get churn data
	churnAnalyzer := churn.New(churn.WithDays(a.churnDays))
	churnAnalysis, err := churnAnalyzer.AnalyzeRepo(ctx, repoPath)
	if err != nil {
		return nil, err
	}

	// Build churn lookup map
	churnMap := make(map[string]*churn.FileMetrics)
	for i := range churnAnalysis.Files {
		f := &churnAnalysis.Files[i]
		// Normalize paths for matching
		churnMap[f.RelativePath] = f
		churnMap[f.Path] = f
	}

	// Analyze complexity in parallel
	type complexityResult struct {
		path          string
		avgCognitive  float64
		avgCyclomatic float64
		totalFuncs    int
	}

	complexityResults := fileproc.MapFilesWithSizeLimit(files, a.maxFileSize, func(psr *parser.Parser, path string) (complexityResult, error) {
		result, err := psr.ParseFile(path)
		if err != nil {
			return complexityResult{path: path}, nil // Skip unparseable files
		}

		functions := parser.GetFunctions(result)
		if len(functions) == 0 {
			return complexityResult{path: path}, nil
		}

		var totalCog, totalCyc uint32
		for _, fn := range functions {
			if fn.Body != nil {
				totalCyc += 1 + complexity.CountDecisionPoints(fn.Body, result.Source, result.Language)
				totalCog += complexity.CalculateCognitiveComplexity(fn.Body, result.Source, result.Language, 0)
			} else {
				totalCyc++
			}
		}

		return complexityResult{
			path:          path,
			avgCognitive:  float64(totalCog) / float64(len(functions)),
			avgCyclomatic: float64(totalCyc) / float64(len(functions)),
			totalFuncs:    len(functions),
		}, nil
	}, onProgress, nil)

	// Combine into hotspot scores using CDF-normalized values
	analysis := &Analysis{
		GeneratedAt: time.Now().UTC(),
		PeriodDays:  a.churnDays,
		Files:       make([]FileHotspot, 0, len(files)),
	}

	for _, cr := range complexityResults {
		// Get relative path for churn lookup
		relPath, err := filepath.Rel(repoPath, cr.path)
		if err != nil {
			relPath = cr.path
		}

		// Look up churn data (commit count)
		var commits int
		if cf, ok := churnMap[relPath]; ok {
			commits = cf.Commits
		} else if cf, ok := churnMap["./"+relPath]; ok {
			commits = cf.Commits
		}

		// Normalize using empirical CDFs (industry benchmarks)
		churnScore := NormalizeChurnCDF(commits)
		complexityScore := NormalizeComplexityCDF(cr.avgCognitive)

		// Hotspot = geometric mean of normalized scores
		// This preserves intersection semantics: both factors must be elevated
		hotspotScore := CalculateScore(churnScore, complexityScore)

		analysis.Files = append(analysis.Files, FileHotspot{
			Path:            cr.path,
			HotspotScore:    hotspotScore,
			ChurnScore:      churnScore,
			ComplexityScore: complexityScore,
			Commits:         commits,
			AvgCognitive:    cr.avgCognitive,
			AvgCyclomatic:   cr.avgCyclomatic,
			TotalFunctions:  cr.totalFuncs,
		})
	}

	// Sort by hotspot score (highest first)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].HotspotScore > analysis.Files[j].HotspotScore
	})

	analysis.CalculateSummary()

	return analysis, nil
}

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	// No resources to release - parsing is handled by fileproc.MapFilesWithProgress
}
