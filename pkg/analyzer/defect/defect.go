package defect

import (
	"context"
	"sort"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/analyzer/churn"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/ownership"
	"github.com/panbanda/omen/pkg/source"
	"github.com/panbanda/omen/pkg/stats"
	"github.com/sourcegraph/conc"
)

// Analyzer predicts defect probability using PMAT weights.
type Analyzer struct {
	weights     Weights
	churnDays   int
	maxFileSize int64
	complexity  *complexity.Analyzer
	churn       *churn.Analyzer
	duplicates  *duplicates.Analyzer
	ownership   *ownership.Analyzer
}

// Compile-time check that Analyzer implements RepoAnalyzer.
var _ analyzer.RepoAnalyzer[*Analysis] = (*Analyzer)(nil)

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithChurnDays sets the churn analysis period in days.
func WithChurnDays(days int) Option {
	return func(a *Analyzer) {
		a.churnDays = days
	}
}

// WithWeights sets custom PMAT weights for defect prediction.
func WithWeights(weights Weights) Option {
	return func(a *Analyzer) {
		a.weights = weights
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// New creates a new defect analyzer with default PMAT weights.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		weights:     DefaultWeights(),
		churnDays:   30,
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}

	// Create sub-analyzers with configured options
	a.complexity = complexity.New(complexity.WithMaxFileSize(a.maxFileSize))
	a.churn = churn.New(churn.WithDays(a.churnDays))
	a.duplicates = duplicates.New(
		duplicates.WithMinTokens(6),
		duplicates.WithSimilarityThreshold(0.8),
		duplicates.WithMaxFileSize(a.maxFileSize),
	)
	a.ownership = ownership.New()

	return a
}

// Analyze predicts defects across a repository.
// If files is nil or empty, all files in the repository are analyzed.
func (a *Analyzer) Analyze(ctx context.Context, repoPath string, files []string) (*Analysis, error) {
	analysis := &Analysis{
		Files:   make([]Score, 0),
		Weights: a.weights,
	}

	// Run sub-analyzers in parallel using conc
	var complexityAnalysis *complexity.Analysis
	var churnAnalysis *churn.Analysis
	var dupAnalysis *duplicates.Analysis
	var ownershipAnalysis *ownership.Analysis
	var complexityErr, churnErr, dupErr, ownershipErr error

	wg := conc.NewWaitGroup()

	fsSrc := source.NewFilesystem()

	// Get complexity metrics
	wg.Go(func() {
		complexityAnalysis, complexityErr = a.complexity.Analyze(ctx, files, fsSrc)
	})

	// Get churn metrics
	wg.Go(func() {
		churnAnalysis, churnErr = a.churn.Analyze(ctx, repoPath, files)
	})

	// Get duplicate metrics
	wg.Go(func() {
		dupAnalysis, dupErr = a.duplicates.Analyze(ctx, files, fsSrc)
	})

	// Get ownership metrics (Bird et al. 2011 - ownership correlates with defects)
	wg.Go(func() {
		ownershipAnalysis, ownershipErr = a.ownership.Analyze(ctx, repoPath, files)
	})

	wg.Wait()

	// Handle complexity error (required)
	if complexityErr != nil {
		return nil, complexityErr
	}

	// Handle churn error (optional - might fail if not a git repo)
	if churnErr != nil {
		churnAnalysis = &churn.Analysis{Files: []churn.FileMetrics{}}
	}

	// Handle duplicate error (optional)
	if dupErr != nil {
		dupAnalysis = &duplicates.Analysis{}
	}

	// Handle ownership error (optional - might fail if not a git repo)
	if ownershipErr != nil {
		ownershipAnalysis = &ownership.Analysis{Files: []ownership.FileOwnership{}}
	}

	// Build lookup maps
	complexityByFile := make(map[string]*complexity.FileResult)
	for i := range complexityAnalysis.Files {
		fc := &complexityAnalysis.Files[i]
		complexityByFile[fc.Path] = fc
	}

	churnByFile := make(map[string]*churn.FileMetrics)
	for i := range churnAnalysis.Files {
		fm := &churnAnalysis.Files[i]
		churnByFile[fm.Path] = fm
	}

	dupByFile := make(map[string]float32)
	for _, clone := range dupAnalysis.Clones {
		dupByFile[clone.FileA] += float32(clone.LinesA)
		if clone.FileA != clone.FileB {
			dupByFile[clone.FileB] += float32(clone.LinesB)
		}
	}

	ownershipByFile := make(map[string]*ownership.FileOwnership)
	for i := range ownershipAnalysis.Files {
		fo := &ownershipAnalysis.Files[i]
		ownershipByFile[fo.Path] = fo
	}

	// Calculate defect probability for each file
	var totalProb float32
	var highCount, medCount, lowCount int

	for _, path := range files {
		metrics := FileMetrics{
			FilePath: path,
		}

		var fileComplexity *complexity.FileResult

		// Get complexity
		if fc, ok := complexityByFile[path]; ok {
			fileComplexity = fc
			metrics.Complexity = float32(fc.AvgCyclomatic)
			if len(fc.Functions) > 0 {
				var maxCyc uint32
				for _, fn := range fc.Functions {
					if fn.Metrics.Cyclomatic > maxCyc {
						maxCyc = fn.Metrics.Cyclomatic
					}
				}
				metrics.CyclomaticComplexity = maxCyc
			}
		}

		// Get churn
		if cm, ok := churnByFile[path]; ok {
			metrics.ChurnScore = float32(cm.ChurnScore)
		}

		// Get duplication ratio
		if dupLines, ok := dupByFile[path]; ok {
			// Estimate total lines (rough)
			if fileComplexity != nil && len(fileComplexity.Functions) > 0 {
				totalLines := 0
				for _, fn := range fileComplexity.Functions {
					totalLines += fn.Metrics.Lines
				}
				if totalLines > 0 {
					metrics.DuplicateRatio = dupLines / float32(totalLines)
					if metrics.DuplicateRatio > 1 {
						metrics.DuplicateRatio = 1
					}
				}
			}
		}

		// Get ownership diffusion (Bird et al. 2011 - more contributors = higher defect risk)
		if fo, ok := ownershipByFile[path]; ok {
			metrics.OwnershipDiffusion = float32(len(fo.Contributors))
			metrics.OwnershipConcentrat = float32(fo.Concentration)
		}

		// Calculate probability and confidence
		prob := CalculateProbability(metrics, a.weights)
		conf := CalculateConfidence(metrics)
		risk := CalculateRiskLevel(prob)

		// Calculate normalized contributing factors (PMAT-compatible)
		churnNorm := NormalizeChurn(metrics.ChurnScore)
		complexityNorm := NormalizeComplexity(metrics.Complexity)
		duplicateNorm := NormalizeDuplication(metrics.DuplicateRatio)
		couplingNorm := NormalizeCoupling(metrics.AfferentCoupling)
		ownershipNorm := NormalizeOwnership(metrics.OwnershipDiffusion)

		score := Score{
			FilePath:    path,
			Probability: prob,
			Confidence:  conf,
			RiskLevel:   risk,
			ContributingFactors: map[string]float32{
				"churn":       churnNorm * a.weights.Churn,
				"complexity":  complexityNorm * a.weights.Complexity,
				"duplication": duplicateNorm * a.weights.Duplication,
				"coupling":    couplingNorm * a.weights.Coupling,
				"ownership":   ownershipNorm * a.weights.Ownership,
			},
			Recommendations: generateRecommendations(metrics, prob),
		}

		analysis.Files = append(analysis.Files, score)
		totalProb += prob

		switch risk {
		case RiskHigh:
			highCount++
		case RiskMedium:
			medCount++
		case RiskLow:
			lowCount++
		}
	}

	// Build summary
	analysis.Summary = Summary{
		TotalFiles:      len(analysis.Files),
		HighRiskCount:   highCount,
		MediumRiskCount: medCount,
		LowRiskCount:    lowCount,
	}
	if len(analysis.Files) > 0 {
		analysis.Summary.AvgProbability = totalProb / float32(len(analysis.Files))

		// Calculate percentiles
		probs := make([]float64, len(analysis.Files))
		for i, f := range analysis.Files {
			probs[i] = float64(f.Probability)
		}
		sort.Float64s(probs)
		analysis.Summary.P50Probability = float32(stats.Percentile(probs, 50))
		analysis.Summary.P95Probability = float32(stats.Percentile(probs, 95))
	}

	return analysis, nil
}

// generateRecommendations suggests improvements based on metrics.
func generateRecommendations(m FileMetrics, prob float32) []string {
	var recs []string

	if m.ChurnScore > 0.7 {
		recs = append(recs, "High churn detected. Consider stabilizing this file with better test coverage.")
	}

	if m.Complexity > 20 {
		recs = append(recs, "High complexity. Consider refactoring into smaller functions.")
	}

	if m.DuplicateRatio > 0.2 {
		recs = append(recs, "Significant code duplication. Extract common logic into shared functions.")
	}

	if m.CyclomaticComplexity > 15 {
		recs = append(recs, "Complex control flow. Simplify conditional logic or extract helper functions.")
	}

	// Ownership recommendations (Bird et al. 2011)
	if m.OwnershipDiffusion >= 8 {
		recs = append(recs, "High contributor diffusion. Consider establishing clearer ownership or assigning a primary maintainer.")
	} else if m.OwnershipDiffusion >= 5 {
		recs = append(recs, "Moderate contributor diffusion. Document ownership responsibilities.")
	}

	if prob > 0.8 {
		recs = append(recs, "CRITICAL: This file has very high defect probability. Prioritize review and testing.")
	} else if prob > 0.6 {
		recs = append(recs, "HIGH RISK: Schedule a code review and add comprehensive tests.")
	}

	if len(recs) == 0 {
		recs = append(recs, "No immediate concerns. Continue monitoring metrics.")
	}

	return recs
}

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	a.complexity.Close()
	a.duplicates.Close()
	a.ownership.Close()
}
