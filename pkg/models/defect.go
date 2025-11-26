package models

// RiskLevel represents the defect probability risk category.
type RiskLevel string

const (
	RiskLow      RiskLevel = "low"      // < 0.4
	RiskMedium   RiskLevel = "medium"   // 0.4 - 0.6
	RiskHigh     RiskLevel = "high"     // 0.6 - 0.8
	RiskCritical RiskLevel = "critical" // > 0.8
)

// DefectWeights defines the weights for defect prediction factors.
// Based on empirical research (PMAT approach).
type DefectWeights struct {
	Churn       float32 `json:"churn"`       // 0.35
	Complexity  float32 `json:"complexity"`  // 0.30
	Duplication float32 `json:"duplication"` // 0.25
	Coupling    float32 `json:"coupling"`    // 0.10
}

// DefaultDefectWeights returns the standard weights.
func DefaultDefectWeights() DefectWeights {
	return DefectWeights{
		Churn:       0.35,
		Complexity:  0.30,
		Duplication: 0.25,
		Coupling:    0.10,
	}
}

// FileMetrics contains input metrics for defect prediction.
type FileMetrics struct {
	FilePath             string  `json:"file_path"`
	ChurnScore           float32 `json:"churn_score"`       // 0.0 to 1.0
	Complexity           float32 `json:"complexity"`        // Raw complexity
	DuplicateRatio       float32 `json:"duplicate_ratio"`   // 0.0 to 1.0
	AfferentCoupling     float32 `json:"afferent_coupling"` // Incoming deps
	EfferentCoupling     float32 `json:"efferent_coupling"` // Outgoing deps
	LinesOfCode          int     `json:"lines_of_code"`
	CyclomaticComplexity uint32  `json:"cyclomatic_complexity"`
	CognitiveComplexity  uint32  `json:"cognitive_complexity"`
}

// DefectScore represents the prediction result for a file.
type DefectScore struct {
	FilePath            string             `json:"file_path"`
	Probability         float32            `json:"probability"` // 0.0 to 1.0
	RiskLevel           RiskLevel          `json:"risk_level"`
	ContributingFactors map[string]float32 `json:"contributing_factors"`
	Recommendations     []string           `json:"recommendations"`
}

// DefectAnalysis represents the full defect prediction result.
type DefectAnalysis struct {
	Files   []DefectScore `json:"files"`
	Summary DefectSummary `json:"summary"`
	Weights DefectWeights `json:"weights"`
}

// DefectSummary provides aggregate statistics.
type DefectSummary struct {
	TotalFiles      int     `json:"total_files"`
	HighRiskCount   int     `json:"high_risk_count"`
	MediumRiskCount int     `json:"medium_risk_count"`
	LowRiskCount    int     `json:"low_risk_count"`
	AvgProbability  float32 `json:"avg_probability"`
	P50Probability  float32 `json:"p50_probability"`
	P95Probability  float32 `json:"p95_probability"`
}

// CalculateRiskLevel determines risk level from probability.
func CalculateRiskLevel(probability float32) RiskLevel {
	switch {
	case probability > 0.8:
		return RiskCritical
	case probability > 0.6:
		return RiskHigh
	case probability > 0.4:
		return RiskMedium
	default:
		return RiskLow
	}
}

// CalculateProbability computes defect probability from metrics.
func CalculateProbability(m FileMetrics, w DefectWeights) float32 {
	// Normalize complexity (assuming max of 100)
	normalizedComplexity := m.Complexity / 100.0
	if normalizedComplexity > 1.0 {
		normalizedComplexity = 1.0
	}

	// Normalize coupling (assuming max of 50 dependencies)
	totalCoupling := (m.AfferentCoupling + m.EfferentCoupling) / 100.0
	if totalCoupling > 1.0 {
		totalCoupling = 1.0
	}

	// Weighted sum
	probability := w.Churn*m.ChurnScore +
		w.Complexity*normalizedComplexity +
		w.Duplication*m.DuplicateRatio +
		w.Coupling*totalCoupling

	if probability > 1.0 {
		probability = 1.0
	}

	return probability
}
