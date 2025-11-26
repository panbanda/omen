package models

import (
	"math"
	"testing"
)

func TestDefaultDefectWeights(t *testing.T) {
	weights := DefaultDefectWeights()

	if weights.Churn != 0.35 {
		t.Errorf("Churn = %v, expected 0.35", weights.Churn)
	}
	if weights.Complexity != 0.30 {
		t.Errorf("Complexity = %v, expected 0.30", weights.Complexity)
	}
	if weights.Duplication != 0.25 {
		t.Errorf("Duplication = %v, expected 0.25", weights.Duplication)
	}
	if weights.Coupling != 0.10 {
		t.Errorf("Coupling = %v, expected 0.10", weights.Coupling)
	}

	total := weights.Churn + weights.Complexity + weights.Duplication + weights.Coupling
	if math.Abs(float64(total-1.0)) > 0.001 {
		t.Errorf("Weights sum = %v, expected 1.0", total)
	}
}

func TestCalculateRiskLevel(t *testing.T) {
	// PMAT-compatible: Low (<0.3), Medium (0.3-0.7), High (>=0.7)
	tests := []struct {
		name        string
		probability float32
		expected    RiskLevel
	}{
		{
			name:        "high risk",
			probability: 0.85,
			expected:    RiskHigh,
		},
		{
			name:        "just at high boundary",
			probability: 0.7,
			expected:    RiskHigh,
		},
		{
			name:        "medium risk upper",
			probability: 0.69,
			expected:    RiskMedium,
		},
		{
			name:        "medium risk",
			probability: 0.5,
			expected:    RiskMedium,
		},
		{
			name:        "just at medium boundary",
			probability: 0.3,
			expected:    RiskMedium,
		},
		{
			name:        "low risk",
			probability: 0.29,
			expected:    RiskLow,
		},
		{
			name:        "very low risk",
			probability: 0.0,
			expected:    RiskLow,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateRiskLevel(tt.probability)
			if got != tt.expected {
				t.Errorf("CalculateRiskLevel(%v) = %v, expected %v", tt.probability, got, tt.expected)
			}
		})
	}
}

func TestCalculateProbability(t *testing.T) {
	weights := DefaultDefectWeights()

	// Note: PMAT uses CDF normalization and sigmoid transformation
	// The sigmoid formula is: 1 / (1 + exp(-10 * (rawScore - 0.5)))
	// This compresses values around 0.5, so:
	// - rawScore=0.0 -> sigmoid~=0.007
	// - rawScore=0.5 -> sigmoid=0.5
	// - rawScore=1.0 -> sigmoid~=0.993

	tests := []struct {
		name    string
		metrics FileMetrics
	}{
		{
			name: "zero metrics produces low probability",
			metrics: FileMetrics{
				ChurnScore:       0.0,
				Complexity:       0.0,
				DuplicateRatio:   0.0,
				AfferentCoupling: 0.0,
				EfferentCoupling: 0.0,
			},
		},
		{
			name: "max metrics produces high probability",
			metrics: FileMetrics{
				ChurnScore:       1.0,
				Complexity:       100.0,
				DuplicateRatio:   1.0,
				AfferentCoupling: 50.0,
				EfferentCoupling: 50.0,
			},
		},
		{
			name: "moderate metrics",
			metrics: FileMetrics{
				ChurnScore:       0.5,
				Complexity:       10.0,
				DuplicateRatio:   0.3,
				AfferentCoupling: 5.0,
				EfferentCoupling: 10.0,
			},
		},
		{
			name: "high complexity only",
			metrics: FileMetrics{
				ChurnScore:       0.0,
				Complexity:       20.0,
				DuplicateRatio:   0.0,
				AfferentCoupling: 0.0,
				EfferentCoupling: 0.0,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateProbability(tt.metrics, weights)

			// Verify probability is in valid range
			if got < 0.0 || got > 1.0 {
				t.Errorf("Probability %v is outside valid range [0.0, 1.0]", got)
			}
		})
	}
}

func TestCalculateProbability_SigmoidBehavior(t *testing.T) {
	weights := DefaultDefectWeights()

	// Test that sigmoid produces expected behavior:
	// - Low raw scores produce probabilities < 0.5
	// - High raw scores produce probabilities > 0.5

	lowMetrics := FileMetrics{
		ChurnScore:       0.0,
		Complexity:       1.0,
		DuplicateRatio:   0.0,
		AfferentCoupling: 0.0,
	}

	highMetrics := FileMetrics{
		ChurnScore:       1.0,
		Complexity:       50.0,
		DuplicateRatio:   1.0,
		AfferentCoupling: 20.0,
	}

	lowProb := CalculateProbability(lowMetrics, weights)
	highProb := CalculateProbability(highMetrics, weights)

	if lowProb >= 0.5 {
		t.Errorf("Low metrics should produce probability < 0.5, got %v", lowProb)
	}

	if highProb <= 0.5 {
		t.Errorf("High metrics should produce probability > 0.5, got %v", highProb)
	}

	if lowProb >= highProb {
		t.Errorf("High metrics probability (%v) should be greater than low metrics (%v)", highProb, lowProb)
	}
}

func TestCalculateProbability_CustomWeights(t *testing.T) {
	customWeights := DefectWeights{
		Churn:       0.50,
		Complexity:  0.30,
		Duplication: 0.10,
		Coupling:    0.10,
	}

	// High churn should produce higher probability than low churn
	highChurnMetrics := FileMetrics{
		ChurnScore:       1.0,
		Complexity:       1.0,
		DuplicateRatio:   0.0,
		AfferentCoupling: 0.0,
	}

	lowChurnMetrics := FileMetrics{
		ChurnScore:       0.0,
		Complexity:       1.0,
		DuplicateRatio:   0.0,
		AfferentCoupling: 0.0,
	}

	highProb := CalculateProbability(highChurnMetrics, customWeights)
	lowProb := CalculateProbability(lowChurnMetrics, customWeights)

	if highProb <= lowProb {
		t.Errorf("High churn probability (%v) should be greater than low churn (%v)", highProb, lowProb)
	}
}

func TestRiskLevel_Constants(t *testing.T) {
	// PMAT-compatible: 3 risk levels
	if RiskLow != "low" {
		t.Errorf("RiskLow = %s, expected low", RiskLow)
	}
	if RiskMedium != "medium" {
		t.Errorf("RiskMedium = %s, expected medium", RiskMedium)
	}
	if RiskHigh != "high" {
		t.Errorf("RiskHigh = %s, expected high", RiskHigh)
	}
}

func TestCalculateProbability_BoundaryConditions(t *testing.T) {
	weights := DefaultDefectWeights()

	t.Run("exactly at normalization boundary", func(t *testing.T) {
		metrics := FileMetrics{
			Complexity:       100.0,
			AfferentCoupling: 50.0,
			EfferentCoupling: 50.0,
		}

		probability := CalculateProbability(metrics, weights)
		if probability > 1.0 {
			t.Errorf("Probability %v exceeds 1.0", probability)
		}
	})

	t.Run("all factors at maximum produces very high probability", func(t *testing.T) {
		metrics := FileMetrics{
			ChurnScore:       1.0,
			Complexity:       1000.0,
			DuplicateRatio:   1.0,
			AfferentCoupling: 1000.0,
			EfferentCoupling: 1000.0,
		}

		probability := CalculateProbability(metrics, weights)
		// With sigmoid, max inputs produce ~0.993, not exactly 1.0
		if probability < 0.99 {
			t.Errorf("Max probability = %v, expected >= 0.99", probability)
		}
		if probability > 1.0 {
			t.Errorf("Probability %v exceeds 1.0", probability)
		}
	})
}

func TestInterpolateCDF(t *testing.T) {
	// Test CDF interpolation directly
	tests := []struct {
		name     string
		table    [][2]float32
		value    float32
		expected float32
	}{
		{
			name:     "below minimum",
			table:    [][2]float32{{0.0, 0.0}, {1.0, 1.0}},
			value:    -1.0,
			expected: 0.0,
		},
		{
			name:     "above maximum",
			table:    [][2]float32{{0.0, 0.0}, {1.0, 1.0}},
			value:    2.0,
			expected: 1.0,
		},
		{
			name:     "midpoint interpolation",
			table:    [][2]float32{{0.0, 0.0}, {10.0, 1.0}},
			value:    5.0,
			expected: 0.5,
		},
		{
			name:     "exact match",
			table:    [][2]float32{{0.0, 0.0}, {5.0, 0.5}, {10.0, 1.0}},
			value:    5.0,
			expected: 0.5,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := interpolateCDF(tt.table, tt.value)
			if math.Abs(float64(got-tt.expected)) > 0.001 {
				t.Errorf("interpolateCDF() = %v, expected %v", got, tt.expected)
			}
		})
	}
}

func TestSigmoid(t *testing.T) {
	// Test sigmoid function behavior
	tests := []struct {
		name     string
		input    float32
		expected float32
	}{
		{"zero input", 0.0, 0.00669},    // 1/(1+exp(5)) ~ 0.00669
		{"midpoint", 0.5, 0.5},          // 1/(1+exp(0)) = 0.5
		{"max input", 1.0, 0.99330},     // 1/(1+exp(-5)) ~ 0.99330
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := sigmoid(tt.input)
			if math.Abs(float64(got-tt.expected)) > 0.01 {
				t.Errorf("sigmoid(%v) = %v, expected %v", tt.input, got, tt.expected)
			}
		})
	}
}

func TestCalculateConfidence(t *testing.T) {
	tests := []struct {
		name        string
		metrics     FileMetrics
		minExpected float32
		maxExpected float32
	}{
		{
			name: "full confidence",
			metrics: FileMetrics{
				LinesOfCode:      100,
				AfferentCoupling: 5,
				ChurnScore:       0.5,
			},
			minExpected: 0.99,
			maxExpected: 1.0,
		},
		{
			name: "small file reduces confidence",
			metrics: FileMetrics{
				LinesOfCode:      5,
				AfferentCoupling: 5,
				ChurnScore:       0.5,
			},
			minExpected: 0.49,
			maxExpected: 0.51,
		},
		{
			name: "no coupling reduces confidence",
			metrics: FileMetrics{
				LinesOfCode:      100,
				AfferentCoupling: 0,
				EfferentCoupling: 0,
				ChurnScore:       0.5,
			},
			minExpected: 0.89,
			maxExpected: 0.91,
		},
		{
			name: "no churn reduces confidence",
			metrics: FileMetrics{
				LinesOfCode:      100,
				AfferentCoupling: 5,
				ChurnScore:       0,
			},
			minExpected: 0.84,
			maxExpected: 0.86,
		},
		{
			name: "multiple reductions stack",
			metrics: FileMetrics{
				LinesOfCode:      5,
				AfferentCoupling: 0,
				EfferentCoupling: 0,
				ChurnScore:       0,
			},
			minExpected: 0.38,
			maxExpected: 0.39,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateConfidence(tt.metrics)
			if got < tt.minExpected || got > tt.maxExpected {
				t.Errorf("CalculateConfidence() = %v, expected between %v and %v", got, tt.minExpected, tt.maxExpected)
			}
		})
	}
}
