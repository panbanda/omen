package analyzer

import (
	"sort"

	"github.com/panbanda/omen/pkg/models"
)

// DefectAnalyzer predicts defect probability using PMAT weights.
type DefectAnalyzer struct {
	weights    models.DefectWeights
	complexity *ComplexityAnalyzer
	churn      *ChurnAnalyzer
	duplicates *DuplicateAnalyzer
}

// NewDefectAnalyzer creates a new defect analyzer with default PMAT weights.
func NewDefectAnalyzer(churnDays int) *DefectAnalyzer {
	return &DefectAnalyzer{
		weights:    models.DefaultDefectWeights(),
		complexity: NewComplexityAnalyzer(),
		churn:      NewChurnAnalyzer(churnDays),
		duplicates: NewDuplicateAnalyzer(6, 0.8),
	}
}

// AnalyzeProject predicts defects across a project.
func (a *DefectAnalyzer) AnalyzeProject(repoPath string, files []string) (*models.DefectAnalysis, error) {
	analysis := &models.DefectAnalysis{
		Files:   make([]models.DefectScore, 0),
		Weights: a.weights,
	}

	// Get complexity metrics
	complexityAnalysis, err := a.complexity.AnalyzeProject(files)
	if err != nil {
		return nil, err
	}
	complexityByFile := make(map[string]*models.FileComplexity)
	for i := range complexityAnalysis.Files {
		fc := &complexityAnalysis.Files[i]
		complexityByFile[fc.Path] = fc
	}

	// Get churn metrics
	churnAnalysis, err := a.churn.AnalyzeFiles(repoPath, files)
	if err != nil {
		// Churn analysis might fail if not a git repo
		churnAnalysis = &models.ChurnAnalysis{Files: []models.FileChurnMetrics{}}
	}
	churnByFile := make(map[string]*models.FileChurnMetrics)
	for i := range churnAnalysis.Files {
		fm := &churnAnalysis.Files[i]
		churnByFile[fm.Path] = fm
	}

	// Get duplicate metrics
	dupAnalysis, err := a.duplicates.AnalyzeProject(files)
	if err != nil {
		dupAnalysis = &models.CloneAnalysis{}
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

		// Get complexity
		if fc, ok := complexityByFile[path]; ok {
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
			if fc, ok := complexityByFile[path]; ok && len(fc.Functions) > 0 {
				totalLines := 0
				for _, fn := range fc.Functions {
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

		// Calculate probability
		prob := models.CalculateProbability(metrics, a.weights)
		risk := models.CalculateRiskLevel(prob)

		score := models.DefectScore{
			FilePath:    path,
			Probability: prob,
			RiskLevel:   risk,
			ContributingFactors: map[string]float32{
				"churn":       metrics.ChurnScore * a.weights.Churn,
				"complexity":  metrics.Complexity / 100 * a.weights.Complexity,
				"duplication": metrics.DuplicateRatio * a.weights.Duplication,
				"coupling":    (metrics.AfferentCoupling + metrics.EfferentCoupling) / 100 * a.weights.Coupling,
			},
			Recommendations: generateRecommendations(metrics, prob),
		}

		analysis.Files = append(analysis.Files, score)
		totalProb += prob

		switch risk {
		case models.RiskHigh, models.RiskCritical:
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
