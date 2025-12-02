package analyzer

import (
	"sort"

	"github.com/panbanda/omen/pkg/models"
	"github.com/sourcegraph/conc"
)

// DefectAnalyzer predicts defect probability using PMAT weights.
type DefectAnalyzer struct {
	weights     models.DefectWeights
	churnDays   int
	maxFileSize int64
	complexity  *ComplexityAnalyzer
	churn       *ChurnAnalyzer
	duplicates  *DuplicateAnalyzer
}

// DefectOption is a functional option for configuring DefectAnalyzer.
type DefectOption func(*DefectAnalyzer)

// WithDefectChurnDays sets the churn analysis period in days.
func WithDefectChurnDays(days int) DefectOption {
	return func(a *DefectAnalyzer) {
		a.churnDays = days
	}
}

// WithDefectWeights sets custom PMAT weights for defect prediction.
func WithDefectWeights(weights models.DefectWeights) DefectOption {
	return func(a *DefectAnalyzer) {
		a.weights = weights
	}
}

// WithDefectMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithDefectMaxFileSize(maxSize int64) DefectOption {
	return func(a *DefectAnalyzer) {
		a.maxFileSize = maxSize
	}
}

// NewDefectAnalyzer creates a new defect analyzer with default PMAT weights.
func NewDefectAnalyzer(opts ...DefectOption) *DefectAnalyzer {
	a := &DefectAnalyzer{
		weights:     models.DefaultDefectWeights(),
		churnDays:   30,
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}

	// Create sub-analyzers with configured options
	a.complexity = NewComplexityAnalyzer(WithComplexityMaxFileSize(a.maxFileSize))
	a.churn = NewChurnAnalyzer(WithChurnDays(a.churnDays))
	a.duplicates = NewDuplicateAnalyzer(
		WithDuplicateMinTokens(6),
		WithDuplicateSimilarityThreshold(0.8),
		WithDuplicateMaxFileSize(a.maxFileSize),
	)

	return a
}

// AnalyzeProject predicts defects across a project.
func (a *DefectAnalyzer) AnalyzeProject(repoPath string, files []string) (*models.DefectAnalysis, error) {
	analysis := &models.DefectAnalysis{
		Files:   make([]models.DefectScore, 0),
		Weights: a.weights,
	}

	// Run sub-analyzers in parallel using conc
	var complexityAnalysis *models.ComplexityAnalysis
	var churnAnalysis *models.ChurnAnalysis
	var dupAnalysis *models.CloneAnalysis
	var complexityErr, churnErr, dupErr error

	wg := conc.NewWaitGroup()

	// Get complexity metrics
	wg.Go(func() {
		complexityAnalysis, complexityErr = a.complexity.AnalyzeProject(files)
	})

	// Get churn metrics
	wg.Go(func() {
		churnAnalysis, churnErr = a.churn.AnalyzeFiles(repoPath, files)
	})

	// Get duplicate metrics
	wg.Go(func() {
		dupAnalysis, dupErr = a.duplicates.AnalyzeProject(files)
	})

	wg.Wait()

	// Handle complexity error (required)
	if complexityErr != nil {
		return nil, complexityErr
	}

	// Handle churn error (optional - might fail if not a git repo)
	if churnErr != nil {
		churnAnalysis = &models.ChurnAnalysis{Files: []models.FileChurnMetrics{}}
	}

	// Handle duplicate error (optional)
	if dupErr != nil {
		dupAnalysis = &models.CloneAnalysis{}
	}

	// Build lookup maps
	complexityByFile := make(map[string]*models.FileComplexity)
	for i := range complexityAnalysis.Files {
		fc := &complexityAnalysis.Files[i]
		complexityByFile[fc.Path] = fc
	}

	churnByFile := make(map[string]*models.FileChurnMetrics)
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

	// Calculate defect probability for each file
	var totalProb float32
	var highCount, medCount, lowCount int

	for _, path := range files {
		metrics := models.FileMetrics{
			FilePath: path,
		}

		var fileComplexity *models.FileComplexity

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

		// Calculate probability and confidence
		prob := models.CalculateProbability(metrics, a.weights)
		conf := models.CalculateConfidence(metrics)
		risk := models.CalculateRiskLevel(prob)

		// Calculate normalized contributing factors (PMAT-compatible)
		churnNorm := models.NormalizeChurn(metrics.ChurnScore)
		complexityNorm := models.NormalizeComplexity(metrics.Complexity)
		duplicateNorm := models.NormalizeDuplication(metrics.DuplicateRatio)
		couplingNorm := models.NormalizeCoupling(metrics.AfferentCoupling)

		score := models.DefectScore{
			FilePath:    path,
			Probability: prob,
			Confidence:  conf,
			RiskLevel:   risk,
			ContributingFactors: map[string]float32{
				"churn":       churnNorm * a.weights.Churn,
				"complexity":  complexityNorm * a.weights.Complexity,
				"duplication": duplicateNorm * a.weights.Duplication,
				"coupling":    couplingNorm * a.weights.Coupling,
			},
			Recommendations: generateRecommendations(metrics, prob),
		}

		analysis.Files = append(analysis.Files, score)
		totalProb += prob

		switch risk {
		case models.RiskHigh:
			highCount++
		case models.RiskMedium:
			medCount++
		case models.RiskLow:
			lowCount++
		}
	}

	// Build summary
	analysis.Summary = models.DefectSummary{
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
		analysis.Summary.P50Probability = float32(percentileFloat64Defect(probs, 50))
		analysis.Summary.P95Probability = float32(percentileFloat64Defect(probs, 95))
	}

	return analysis, nil
}

// percentileFloat64Defect calculates the p-th percentile of a sorted slice.
func percentileFloat64Defect(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// generateRecommendations suggests improvements based on metrics.
func generateRecommendations(m models.FileMetrics, prob float32) []string {
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
func (a *DefectAnalyzer) Close() {
	a.complexity.Close()
	a.duplicates.Close()
}
