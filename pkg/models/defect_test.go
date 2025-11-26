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
	tests := []struct {
		name        string
		probability float32
		expected    RiskLevel
	}{
		{
			name:        "critical risk",
			probability: 0.85,
			expected:    RiskCritical,
		},
		{
			name:        "just above critical boundary",
			probability: 0.81,
			expected:    RiskCritical,
		},
		{
			name:        "high risk",
			probability: 0.7,
			expected:    RiskHigh,
		},
		{
			name:        "just above high boundary",
			probability: 0.61,
			expected:    RiskHigh,
		},
		{
			name:        "medium risk",
			probability: 0.5,
			expected:    RiskMedium,
		},
		{
			name:        "just above medium boundary",
			probability: 0.41,
			expected:    RiskMedium,
		},
		{
			name:        "low risk",
			probability: 0.3,
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

	tests := []struct {
		name     string
		metrics  FileMetrics
		expected float32
		tolerance float32
	}{
		{
			name: "zero metrics",
			metrics: FileMetrics{
				ChurnScore:       0.0,
				Complexity:       0.0,
				DuplicateRatio:   0.0,
				AfferentCoupling: 0.0,
				EfferentCoupling: 0.0,
			},
			expected: 0.0,
			tolerance: 0.01,
		},
		{
			name: "max metrics",
			metrics: FileMetrics{
				ChurnScore:       1.0,
				Complexity:       100.0,
				DuplicateRatio:   1.0,
				AfferentCoupling: 50.0,
				EfferentCoupling: 50.0,
			},
			expected: 1.0,
			tolerance: 0.01,
		},
		{
			name: "moderate risk",
			metrics: FileMetrics{
				ChurnScore:       0.5,
				Complexity:       50.0,
				DuplicateRatio:   0.3,
				AfferentCoupling: 10.0,
				EfferentCoupling: 10.0,
			},
			expected: 0.35*0.5 + 0.30*0.5 + 0.25*0.3 + 0.10*0.2,
			tolerance: 0.01,
		},
		{
			name: "high complexity only",
			metrics: FileMetrics{
				ChurnScore:       0.0,
				Complexity:       80.0,
				DuplicateRatio:   0.0,
				AfferentCoupling: 0.0,
				EfferentCoupling: 0.0,
			},
			expected: 0.30 * 0.8,
			tolerance: 0.01,
		},
		{
			name: "complexity exceeds max",
			metrics: FileMetrics{
				ChurnScore:       0.0,
				Complexity:       200.0,
				DuplicateRatio:   0.0,
				AfferentCoupling: 0.0,
				EfferentCoupling: 0.0,
			},
			expected: 0.30,
			tolerance: 0.01,
		},
		{
			name: "coupling exceeds max",
			metrics: FileMetrics{
				ChurnScore:       0.0,
				Complexity:       0.0,
				DuplicateRatio:   0.0,
				AfferentCoupling: 100.0,
				EfferentCoupling: 100.0,
			},
			expected: 0.10,
			tolerance: 0.01,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateProbability(tt.metrics, weights)

			if math.Abs(float64(got-tt.expected)) > float64(tt.tolerance) {
				t.Errorf("CalculateProbability() = %v, expected %v (±%v)", got, tt.expected, tt.tolerance)
			}

			if got < 0.0 || got > 1.0 {
				t.Errorf("Probability %v is outside valid range [0.0, 1.0]", got)
			}
		})
	}
}

func TestCalculateProbability_CustomWeights(t *testing.T) {
	customWeights := DefectWeights{
		Churn:       0.50,
		Complexity:  0.30,
		Duplication: 0.10,
		Coupling:    0.10,
	}

	metrics := FileMetrics{
		ChurnScore:       1.0,
		Complexity:       0.0,
		DuplicateRatio:   0.0,
		AfferentCoupling: 0.0,
		EfferentCoupling: 0.0,
	}

	probability := CalculateProbability(metrics, customWeights)
	expected := float32(0.50)

	if math.Abs(float64(probability-expected)) > 0.01 {
		t.Errorf("With custom weights, probability = %v, expected %v", probability, expected)
	}
}

func TestRiskLevel_Constants(t *testing.T) {
	if RiskLow != "low" {
		t.Errorf("RiskLow = %s, expected low", RiskLow)
	}
	if RiskMedium != "medium" {
		t.Errorf("RiskMedium = %s, expected medium", RiskMedium)
	}
	if RiskHigh != "high" {
		t.Errorf("RiskHigh = %s, expected high", RiskHigh)
	}
	if RiskCritical != "critical" {
		t.Errorf("RiskCritical = %s, expected critical", RiskCritical)
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

	t.Run("all factors at maximum", func(t *testing.T) {
		metrics := FileMetrics{
			ChurnScore:       1.0,
			Complexity:       1000.0,
			DuplicateRatio:   1.0,
			AfferentCoupling: 1000.0,
			EfferentCoupling: 1000.0,
		}

		probability := CalculateProbability(metrics, weights)
		if probability != 1.0 {
			t.Errorf("Max probability = %v, expected 1.0", probability)
		}
	})
}
