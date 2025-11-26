package analyzer

import (
	"sort"

	"github.com/jonathanreyes/omen-cli/pkg/models"
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
	minScore := 100.0 // Track worst score (lowest)

	for _, path := range files {
		components := models.TDGComponents{}

		// Complexity component (normalized to 0-1)
		if fc, ok := complexityByFile[path]; ok {
			// Use max complexity, normalized by threshold of 20
			var maxCyc float64
			for _, fn := range fc.Functions {
				if float64(fn.Metrics.Cyclomatic) > maxCyc {
					maxCyc = float64(fn.Metrics.Cyclomatic)
				}
			}
			components.Complexity = normalize(maxCyc, 20)
		}

		// Churn component (normalized to 0-1)
		if cm, ok := churnByFile[path]; ok {
			components.Churn = cm.ChurnScore
		}

		// Duplication component (normalized to 0-1)
		if dupLines, ok := dupByFile[path]; ok {
			if fc, ok := complexityByFile[path]; ok {
				totalLines := 0
				for _, fn := range fc.Functions {
					totalLines += fn.Metrics.Lines
				}
				if totalLines > 0 {
					components.Duplication = normalize(dupLines/float64(totalLines), 0.3)
				}
			}
		}

		// Coupling component (placeholder - would need graph analysis)
		components.Coupling = 0

		// Domain risk component (placeholder - would need domain classification)
		components.DomainRisk = 0

		// Calculate TDG score
		score := models.CalculateTDG(components, a.weights)
		severity := models.CalculateTDGSeverity(score)

		tdgScore := models.TDGScore{
			FilePath:   path,
			Value:      score,
			Severity:   severity,
			Components: components,
		}

		analysis.Files = append(analysis.Files, tdgScore)
		totalScore += score
		if score < minScore {
			minScore = score
		}
		analysis.Summary.BySeverity[string(severity)]++
	}

	// Sort by score (lowest first - worst files at top)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].Value < analysis.Files[j].Value
	})

	// Build summary
	analysis.Summary.TotalFiles = len(analysis.Files)
	analysis.Summary.MaxScore = minScore // Store worst (lowest) score
	if len(analysis.Files) > 0 {
		analysis.Summary.AvgScore = totalScore / float64(len(analysis.Files))

		// Calculate percentiles (files already sorted ascending by score)
		scores := make([]float64, len(analysis.Files))
		for i, f := range analysis.Files {
			scores[i] = f.Value
		}
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

// normalize maps a value to 0-1 range based on a threshold.
func normalize(value, threshold float64) float64 {
	if value <= 0 {
		return 0
	}
	if value >= threshold {
		return 1
	}
	return value / threshold
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
