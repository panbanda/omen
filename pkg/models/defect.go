package models

import (
	"fmt"
	"math"
)

// RiskLevel represents the defect probability risk category.
// PMAT-compatible: 3 levels with thresholds at 0.3 and 0.7
type RiskLevel string

const (
	RiskLow    RiskLevel = "low"    // < 0.3
	RiskMedium RiskLevel = "medium" // 0.3 - 0.7
	RiskHigh   RiskLevel = "high"   // >= 0.7
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

// DefectScore represents the prediction result for a file (internal format).
type DefectScore struct {
	FilePath            string             `json:"file_path"`
	Probability         float32            `json:"probability"` // 0.0 to 1.0
	Confidence          float32            `json:"confidence"`  // 0.0 to 1.0
	RiskLevel           RiskLevel          `json:"risk_level"`
	ContributingFactors map[string]float32 `json:"contributing_factors"`
	Recommendations     []string           `json:"recommendations"`
}

// DefectAnalysis represents the full defect prediction result (internal format).
type DefectAnalysis struct {
	Files   []DefectScore `json:"files"`
	Summary DefectSummary `json:"summary"`
	Weights DefectWeights `json:"weights"`
}

// DefectSummary provides aggregate statistics (internal format).
type DefectSummary struct {
	TotalFiles      int     `json:"total_files"`
	HighRiskCount   int     `json:"high_risk_count"`
	MediumRiskCount int     `json:"medium_risk_count"`
	LowRiskCount    int     `json:"low_risk_count"`
	AvgProbability  float32 `json:"avg_probability"`
	P50Probability  float32 `json:"p50_probability"`
	P95Probability  float32 `json:"p95_probability"`
}

// FilePrediction represents a file's defect prediction (pmat-compatible).
type FilePrediction struct {
	FilePath  string   `json:"file_path"`
	RiskScore float32  `json:"risk_score"`
	RiskLevel string   `json:"risk_level"`
	Factors   []string `json:"factors"`
}

// DefectPredictionReport is the pmat-compatible output format.
type DefectPredictionReport struct {
	TotalFiles      int              `json:"total_files"`
	HighRiskFiles   int              `json:"high_risk_files"`
	MediumRiskFiles int              `json:"medium_risk_files"`
	LowRiskFiles    int              `json:"low_risk_files"`
	FilePredictions []FilePrediction `json:"file_predictions"`
}

// ToDefectPredictionReport converts DefectAnalysis to pmat-compatible format.
func (a *DefectAnalysis) ToDefectPredictionReport() *DefectPredictionReport {
	report := &DefectPredictionReport{
		TotalFiles:      a.Summary.TotalFiles,
		HighRiskFiles:   a.Summary.HighRiskCount,
		MediumRiskFiles: a.Summary.MediumRiskCount,
		LowRiskFiles:    a.Summary.LowRiskCount,
		FilePredictions: make([]FilePrediction, 0, len(a.Files)),
	}

	for _, score := range a.Files {
		// Convert contributing factors map to string array like pmat
		factors := make([]string, 0, len(score.ContributingFactors))
		for factor, contribution := range score.ContributingFactors {
			factors = append(factors, fmt.Sprintf("%s: %.1f%%", factor, contribution*100))
		}

		pred := FilePrediction{
			FilePath:  score.FilePath,
			RiskScore: score.Probability,
			RiskLevel: string(score.RiskLevel),
			Factors:   factors,
		}
		report.FilePredictions = append(report.FilePredictions, pred)
	}

	return report
}

// CalculateRiskLevel determines risk level from probability.
// PMAT-compatible: Low (<0.3), Medium (0.3-0.7), High (>=0.7)
func CalculateRiskLevel(probability float32) RiskLevel {
	switch {
	case probability >= 0.7:
		return RiskHigh
	case probability >= 0.3:
		return RiskMedium
	default:
		return RiskLow
	}
}

// PMAT-compatible CDF percentile tables for normalization
var churnPercentiles = [][2]float32{
	{0.0, 0.0}, {0.1, 0.05}, {0.2, 0.15}, {0.3, 0.30},
	{0.4, 0.50}, {0.5, 0.70}, {0.6, 0.85}, {0.7, 0.93},
	{0.8, 0.97}, {1.0, 1.0},
}

var complexityPercentiles = [][2]float32{
	{1.0, 0.1}, {2.0, 0.2}, {3.0, 0.3}, {5.0, 0.5},
	{7.0, 0.7}, {10.0, 0.8}, {15.0, 0.9}, {20.0, 0.95},
	{30.0, 0.98}, {50.0, 1.0},
}

var couplingPercentiles = [][2]float32{
	{0.0, 0.1}, {1.0, 0.3}, {2.0, 0.5}, {3.0, 0.7},
	{5.0, 0.8}, {8.0, 0.9}, {12.0, 0.95}, {20.0, 1.0},
}

// interpolateCDF performs linear interpolation on CDF percentile tables.
func interpolateCDF(percentiles [][2]float32, value float32) float32 {
	if value <= percentiles[0][0] {
		return percentiles[0][1]
	}
	if value >= percentiles[len(percentiles)-1][0] {
		return percentiles[len(percentiles)-1][1]
	}

	for i := 0; i < len(percentiles)-1; i++ {
		x1, y1 := percentiles[i][0], percentiles[i][1]
		x2, y2 := percentiles[i+1][0], percentiles[i+1][1]

		if value >= x1 && value <= x2 {
			t := (value - x1) / (x2 - x1)
			return y1 + t*(y2-y1)
		}
	}
	return 0.0
}

// NormalizeChurn normalizes churn score using empirical CDF from OSS projects.
func NormalizeChurn(rawScore float32) float32 {
	return interpolateCDF(churnPercentiles, rawScore)
}

// NormalizeComplexity normalizes complexity using empirical CDF.
func NormalizeComplexity(rawScore float32) float32 {
	return interpolateCDF(complexityPercentiles, rawScore)
}

// NormalizeDuplication normalizes duplication ratio (direct clamp since it's already a ratio).
func NormalizeDuplication(rawScore float32) float32 {
	if rawScore < 0 {
		return 0
	}
	if rawScore > 1 {
		return 1
	}
	return rawScore
}

// NormalizeCoupling normalizes afferent coupling using empirical CDF.
// PMAT uses afferent coupling only (not sum of both).
func NormalizeCoupling(rawScore float32) float32 {
	return interpolateCDF(couplingPercentiles, rawScore)
}

// sigmoid applies sigmoid transformation for probability calibration.
// Formula: 1 / (1 + exp(-10 * (rawScore - 0.5)))
func sigmoid(rawScore float32) float32 {
	return float32(1.0 / (1.0 + math.Exp(-10.0*(float64(rawScore)-0.5))))
}

// CalculateConfidence computes confidence based on data availability.
func CalculateConfidence(m FileMetrics) float32 {
	confidence := float32(1.0)

	// Reduce confidence for very small files (less reliable metrics)
	if m.LinesOfCode < 10 {
		confidence *= 0.5
	} else if m.LinesOfCode < 50 {
		confidence *= 0.8
	}

	// Reduce confidence if coupling metrics are missing/zero
	if m.AfferentCoupling == 0 && m.EfferentCoupling == 0 {
		confidence *= 0.9
	}

	// Reduce confidence for very new files (no churn history)
	if m.ChurnScore == 0 {
		confidence *= 0.85
	}

	if confidence < 0 {
		return 0
	}
	if confidence > 1 {
		return 1
	}
	return confidence
}

// CalculateProbability computes defect probability from metrics.
// PMAT-compatible: uses CDF normalization and sigmoid transformation.
func CalculateProbability(m FileMetrics, w DefectWeights) float32 {
	// Normalize using empirical CDFs
	churnNorm := NormalizeChurn(m.ChurnScore)
	complexityNorm := NormalizeComplexity(m.Complexity)
	duplicateNorm := NormalizeDuplication(m.DuplicateRatio)
	couplingNorm := NormalizeCoupling(m.AfferentCoupling) // PMAT uses afferent only

	// Weighted linear combination
	rawScore := w.Churn*churnNorm +
		w.Complexity*complexityNorm +
		w.Duplication*duplicateNorm +
		w.Coupling*couplingNorm

	// Apply sigmoid for probability interpretation
	return sigmoid(rawScore)
}
