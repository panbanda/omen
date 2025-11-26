package analyzer

import (
	"path/filepath"
	"sort"
	"strings"

	"github.com/panbanda/omen/pkg/models"
)

// TDGAnalyzer calculates Technical Debt Gradient scores.
type TDGAnalyzer struct {
	weights    models.TDGWeights
	complexity *ComplexityAnalyzer
	churn      *ChurnAnalyzer
	duplicates *DuplicateAnalyzer
}

// NewTDGAnalyzer creates a new TDG analyzer with default weights.
func NewTDGAnalyzer(churnDays int) *TDGAnalyzer {
	return &TDGAnalyzer{
		weights:    models.DefaultTDGWeights(),
		complexity: NewComplexityAnalyzer(),
		churn:      NewChurnAnalyzer(churnDays),
		duplicates: NewDuplicateAnalyzer(6, 0.8),
	}
}

// AnalyzeProject calculates TDG scores for a project.
func (a *TDGAnalyzer) AnalyzeProject(repoPath string, files []string) (*models.TDGAnalysis, error) {
	analysis := &models.TDGAnalysis{
		Files:   make([]models.TDGScore, 0),
		Summary: models.NewTDGSummary(),
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
	dupByFile := make(map[string]float64)
	for _, clone := range dupAnalysis.Clones {
		dupByFile[clone.FileA] += float64(clone.LinesA)
		if clone.FileA != clone.FileB {
			dupByFile[clone.FileB] += float64(clone.LinesB)
		}
	}

	// Calculate TDG for each file
	var totalScore float64
	maxScore := 0.0 // Track worst score (highest debt)

	for _, path := range files {
		components := models.TDGComponents{}
		confidence := 1.0

		// Complexity component (normalized to 0-5 scale)
		if fc, ok := complexityByFile[path]; ok {
			var maxCyc float64
			for _, fn := range fc.Functions {
				if float64(fn.Metrics.Cyclomatic) > maxCyc {
					maxCyc = float64(fn.Metrics.Cyclomatic)
				}
			}
			// Normalize: 20+ cyclomatic = 5.0 (max debt)
			components.Complexity = clamp(maxCyc/4.0, 0, 5)
		}

		// Churn component (normalized to 0-5 scale)
		if cm, ok := churnByFile[path]; ok {
			components.Churn = clamp(cm.ChurnScore*5.0, 0, 5)
		} else {
			confidence *= 0.8 // Reduce confidence when churn data missing
		}

		// Duplication component (normalized to 0-5 scale)
		hasDuplication := false
		if dupLines, ok := dupByFile[path]; ok {
			if fc, ok := complexityByFile[path]; ok {
				totalLines := 0
				for _, fn := range fc.Functions {
					totalLines += fn.Metrics.Lines
				}
				if totalLines > 0 {
					ratio := dupLines / float64(totalLines)
					components.Duplication = clamp(ratio*5.0/0.3, 0, 5)
					hasDuplication = true
				}
			}
		}
		if !hasDuplication {
			confidence *= 0.95
		}

		// Coupling component: import_count/15 + instability*2 + penalty
		if fc, ok := complexityByFile[path]; ok {
			importCount := countImports(fc)
			importFactor := clamp(float64(importCount)/15.0, 0, 2)
			// Instability approximated from function count ratio
			instability := estimateInstability(fc)
			instabilityFactor := instability * 2.0
			complexityPenalty := 0.0
			if importCount > 20 {
				complexityPenalty = 1.0
			}
			components.Coupling = clamp(importFactor+instabilityFactor+complexityPenalty, 0, 5)
		} else {
			confidence *= 0.9
		}

		// Domain risk based on path patterns
		components.DomainRisk = calculateDomainRisk(path)

		// Calculate TDG score
		score := models.CalculateTDG(components, a.weights)
		severity := models.CalculateTDGSeverity(score)

		tdgScore := models.TDGScore{
			FilePath:   path,
			Value:      score,
			Severity:   severity,
			Components: components,
			Confidence: confidence,
		}

		analysis.Files = append(analysis.Files, tdgScore)
		totalScore += score
		if score > maxScore {
			maxScore = score
		}
		analysis.Summary.BySeverity[string(severity)]++
	}

	// Sort by score (highest first - worst debt at top)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].Value > analysis.Files[j].Value
	})

	// Build summary
	analysis.Summary.TotalFiles = len(analysis.Files)
	analysis.Summary.MaxScore = maxScore
	if len(analysis.Files) > 0 {
		analysis.Summary.AvgScore = totalScore / float64(len(analysis.Files))

		// Calculate percentiles (need ascending sort for percentile calculation)
		scores := make([]float64, len(analysis.Files))
		for i, f := range analysis.Files {
			scores[i] = f.Value
		}
		sort.Float64s(scores)
		analysis.Summary.P50Score = percentileFloat64TDG(scores, 50)
		analysis.Summary.P95Score = percentileFloat64TDG(scores, 95)
	}

	// Top hotspots
	hotspotCount := 10
	if len(analysis.Files) < hotspotCount {
		hotspotCount = len(analysis.Files)
	}
	analysis.Summary.Hotspots = make([]models.TDGScore, hotspotCount)
	copy(analysis.Summary.Hotspots, analysis.Files[:hotspotCount])

	return analysis, nil
}

// clamp restricts a value to a range.
func clamp(value, min, max float64) float64 {
	if value < min {
		return min
	}
	if value > max {
		return max
	}
	return value
}

// countImports estimates import count from file complexity data.
func countImports(fc *models.FileComplexity) int {
	// Approximate based on efferent coupling or function dependencies
	// In a real implementation, this would parse imports directly
	return len(fc.Functions) / 2
}

// estimateInstability approximates code instability (0-1).
// Instability = Ce / (Ca + Ce) where Ce=efferent, Ca=afferent coupling.
func estimateInstability(fc *models.FileComplexity) float64 {
	if fc == nil || len(fc.Functions) == 0 {
		return 0.5 // Default moderate instability
	}
	// Approximate: more functions = likely more dependencies = higher instability
	funcCount := float64(len(fc.Functions))
	return clamp(funcCount/20.0, 0, 1)
}

// calculateDomainRisk assigns risk based on file path patterns.
func calculateDomainRisk(path string) float64 {
	lower := strings.ToLower(filepath.ToSlash(path))
	risk := 0.0

	// High-risk domains: auth, crypto, security
	if strings.Contains(lower, "auth") ||
		strings.Contains(lower, "crypto") ||
		strings.Contains(lower, "security") ||
		strings.Contains(lower, "password") ||
		strings.Contains(lower, "token") {
		risk += 2.0
	}

	// Medium-risk domains: database, migration
	if strings.Contains(lower, "database") ||
		strings.Contains(lower, "migration") ||
		strings.Contains(lower, "schema") ||
		strings.Contains(lower, "sql") {
		risk += 1.5
	}

	// Lower-risk domains: api, integration
	if strings.Contains(lower, "api") ||
		strings.Contains(lower, "integration") ||
		strings.Contains(lower, "client") ||
		strings.Contains(lower, "handler") {
		risk += 1.0
	}

	return clamp(risk, 0, 5)
}

// percentileFloat64TDG calculates the p-th percentile of a sorted slice.
func percentileFloat64TDG(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// Close releases analyzer resources.
func (a *TDGAnalyzer) Close() {
	a.complexity.Close()
	a.duplicates.Close()
}
