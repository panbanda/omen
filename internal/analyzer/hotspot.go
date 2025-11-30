package analyzer

import (
	"path/filepath"
	"sort"
	"time"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

// HotspotAnalyzer identifies code hotspots using the geometric mean of
// CDF-normalized churn and complexity scores. This approach:
//   - Uses empirical CDFs to normalize metrics against industry benchmarks
//   - Applies geometric mean to require BOTH factors to be elevated
//   - Produces scores where 0.5+ indicates meaningful hotspots
type HotspotAnalyzer struct {
	churnDays   int
	maxFileSize int64
}

// HotspotOption is a functional option for configuring HotspotAnalyzer.
type HotspotOption func(*HotspotAnalyzer)

// WithHotspotChurnDays sets the number of days of git history to analyze.
func WithHotspotChurnDays(days int) HotspotOption {
	return func(a *HotspotAnalyzer) {
		a.churnDays = days
	}
}

// WithHotspotMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithHotspotMaxFileSize(maxSize int64) HotspotOption {
	return func(a *HotspotAnalyzer) {
		a.maxFileSize = maxSize
	}
}

// NewHotspotAnalyzer creates a new hotspot analyzer.
func NewHotspotAnalyzer(opts ...HotspotOption) *HotspotAnalyzer {
	a := &HotspotAnalyzer{
		churnDays:   30,
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// AnalyzeProject analyzes hotspots for a project.
func (a *HotspotAnalyzer) AnalyzeProject(repoPath string, files []string) (*models.HotspotAnalysis, error) {
	return a.AnalyzeProjectWithProgress(repoPath, files, nil)
}

// AnalyzeProjectWithProgress analyzes hotspots with optional progress callback.
func (a *HotspotAnalyzer) AnalyzeProjectWithProgress(repoPath string, files []string, onProgress fileproc.ProgressFunc) (*models.HotspotAnalysis, error) {
	// Get churn data
	churnAnalyzer := NewChurnAnalyzer(WithChurnDays(a.churnDays))
	churnAnalysis, err := churnAnalyzer.AnalyzeRepo(repoPath)
	if err != nil {
		return nil, err
	}

	// Build churn lookup map
	churnMap := make(map[string]*models.FileChurnMetrics)
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
				totalCyc += 1 + countDecisionPoints(fn.Body, result.Source, result.Language)
				totalCog += calculateCognitiveComplexity(fn.Body, result.Source, result.Language, 0)
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
	analysis := &models.HotspotAnalysis{
		GeneratedAt: time.Now().UTC(),
		PeriodDays:  a.churnDays,
		Files:       make([]models.FileHotspot, 0, len(files)),
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
		churnScore := models.NormalizeChurnCDF(commits)
		complexityScore := models.NormalizeComplexityCDF(cr.avgCognitive)

		// Hotspot = geometric mean of normalized scores
		// This preserves intersection semantics: both factors must be elevated
		hotspotScore := models.CalculateHotspotScore(churnScore, complexityScore)

		analysis.Files = append(analysis.Files, models.FileHotspot{
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
func (a *HotspotAnalyzer) Close() {
	// No resources to release - parsing is handled by fileproc.MapFilesWithProgress
}
