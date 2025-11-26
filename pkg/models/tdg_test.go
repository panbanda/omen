package models

import (
	"math"
	"testing"
)

func TestDefaultTDGWeights(t *testing.T) {
	weights := DefaultTDGWeights()

	if weights.Complexity != 0.30 {
		t.Errorf("Complexity = %v, expected 0.30", weights.Complexity)
	}
	if weights.Churn != 0.25 {
		t.Errorf("Churn = %v, expected 0.25", weights.Churn)
	}
	if weights.Coupling != 0.20 {
		t.Errorf("Coupling = %v, expected 0.20", weights.Coupling)
	}
	if weights.Duplication != 0.15 {
		t.Errorf("Duplication = %v, expected 0.15", weights.Duplication)
	}
	if weights.DomainRisk != 0.10 {
		t.Errorf("DomainRisk = %v, expected 0.10", weights.DomainRisk)
	}

	total := weights.Complexity + weights.Churn + weights.Coupling + weights.Duplication + weights.DomainRisk
	if math.Abs(total-1.0) > 0.001 {
		t.Errorf("Weights sum = %v, expected 1.0", total)
	}
}

func TestCalculateTDGSeverity(t *testing.T) {
	tests := []struct {
		name     string
		score    float64
		expected TDGSeverity
	}{
		{
			name:     "excellent - 100",
			score:    100.0,
			expected: TDGExcellent,
		},
		{
			name:     "excellent - 95",
			score:    95.0,
			expected: TDGExcellent,
		},
		{
			name:     "excellent boundary",
			score:    90.0,
			expected: TDGExcellent,
		},
		{
			name:     "good - 85",
			score:    85.0,
			expected: TDGGood,
		},
		{
			name:     "good boundary",
			score:    70.0,
			expected: TDGGood,
		},
		{
			name:     "moderate - 60",
			score:    60.0,
			expected: TDGModerate,
		},
		{
			name:     "moderate boundary",
			score:    50.0,
			expected: TDGModerate,
		},
		{
			name:     "high risk - 45",
			score:    45.0,
			expected: TDGHighRisk,
		},
		{
			name:     "high risk - 0",
			score:    0.0,
			expected: TDGHighRisk,
		},
		{
			name:     "just below excellent",
			score:    89.9,
			expected: TDGGood,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateTDGSeverity(tt.score)
			if got != tt.expected {
				t.Errorf("CalculateTDGSeverity(%v) = %v, expected %v", tt.score, got, tt.expected)
			}
		})
	}
}

func TestCalculateTDG(t *testing.T) {
	weights := DefaultTDGWeights()

	tests := []struct {
		name       string
		components TDGComponents
		expected   float64
		tolerance  float64
	}{
		{
			name: "no debt - perfect score",
			components: TDGComponents{
				Complexity:  0.0,
				Churn:       0.0,
				Coupling:    0.0,
				Duplication: 0.0,
				DomainRisk:  0.0,
			},
			expected:  100.0,
			tolerance: 0.01,
		},
		{
			name: "maximum debt",
			components: TDGComponents{
				Complexity:  1.0,
				Churn:       1.0,
				Coupling:    1.0,
				Duplication: 1.0,
				DomainRisk:  1.0,
			},
			expected:  0.0,
			tolerance: 0.01,
		},
		{
			name: "moderate debt",
			components: TDGComponents{
				Complexity:  0.5,
				Churn:       0.5,
				Coupling:    0.5,
				Duplication: 0.5,
				DomainRisk:  0.5,
			},
			expected:  50.0,
			tolerance: 0.01,
		},
		{
			name: "high complexity only",
			components: TDGComponents{
				Complexity:  1.0,
				Churn:       0.0,
				Coupling:    0.0,
				Duplication: 0.0,
				DomainRisk:  0.0,
			},
			expected:  70.0,
			tolerance: 0.01,
		},
		{
			name: "mixed penalties",
			components: TDGComponents{
				Complexity:  0.8,
				Churn:       0.3,
				Coupling:    0.2,
				Duplication: 0.5,
				DomainRisk:  0.1,
			},
			expected: (1.0 - (0.8*0.30 + 0.3*0.25 + 0.2*0.20 + 0.5*0.15 + 0.1*0.10)) * 100.0,
			tolerance: 0.01,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateTDG(tt.components, weights)

			if math.Abs(got-tt.expected) > tt.tolerance {
				t.Errorf("CalculateTDG() = %v, expected %v (±%v)", got, tt.expected, tt.tolerance)
			}

			if got < 0.0 || got > 100.0 {
				t.Errorf("TDG score %v is outside valid range [0.0, 100.0]", got)
			}
		})
	}
}

func TestCalculateTDG_CustomWeights(t *testing.T) {
	customWeights := TDGWeights{
		Complexity:  0.60,
		Churn:       0.20,
		Coupling:    0.10,
		Duplication: 0.05,
		DomainRisk:  0.05,
	}

	components := TDGComponents{
		Complexity:  1.0,
		Churn:       0.0,
		Coupling:    0.0,
		Duplication: 0.0,
		DomainRisk:  0.0,
	}

	score := CalculateTDG(components, customWeights)
	expected := 40.0

	if math.Abs(score-expected) > 0.01 {
		t.Errorf("With custom weights, score = %v, expected %v", score, expected)
	}
}

func TestTDGSeverity_Color(t *testing.T) {
	tests := []struct {
		name     string
		severity TDGSeverity
		expected string
	}{
		{
			name:     "excellent",
			severity: TDGExcellent,
			expected: "\033[32m",
		},
		{
			name:     "good",
			severity: TDGGood,
			expected: "\033[33m",
		},
		{
			name:     "moderate",
			severity: TDGModerate,
			expected: "\033[38;5;208m",
		},
		{
			name:     "high risk",
			severity: TDGHighRisk,
			expected: "\033[31m",
		},
		{
			name:     "unknown",
			severity: TDGSeverity("unknown"),
			expected: "\033[0m",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.severity.Color()
			if got != tt.expected {
				t.Errorf("Color() = %q, expected %q", got, tt.expected)
			}
		})
	}
}

func TestNewTDGSummary(t *testing.T) {
	s := NewTDGSummary()

	if s.BySeverity == nil {
		t.Error("BySeverity should be initialized")
	}
	if s.Hotspots == nil {
		t.Error("Hotspots should be initialized")
	}

	if len(s.BySeverity) != 0 {
		t.Error("BySeverity should be empty")
	}
	if len(s.Hotspots) != 0 {
		t.Error("Hotspots should be empty")
	}
}

func TestTDGSeverity_Constants(t *testing.T) {
	if TDGExcellent != "excellent" {
		t.Errorf("TDGExcellent = %s, expected excellent", TDGExcellent)
	}
	if TDGGood != "good" {
		t.Errorf("TDGGood = %s, expected good", TDGGood)
	}
	if TDGModerate != "moderate" {
		t.Errorf("TDGModerate = %s, expected moderate", TDGModerate)
	}
	if TDGHighRisk != "high_risk" {
		t.Errorf("TDGHighRisk = %s, expected high_risk", TDGHighRisk)
	}
}

func TestCalculateTDG_BoundaryConditions(t *testing.T) {
	weights := DefaultTDGWeights()

	t.Run("negative penalties clamped to 0", func(t *testing.T) {
		components := TDGComponents{
			Complexity:  -0.5,
			Churn:       0.0,
			Coupling:    0.0,
			Duplication: 0.0,
			DomainRisk:  0.0,
		}

		score := CalculateTDG(components, weights)
		if score < 0 || score > 100 {
			t.Errorf("Score %v outside valid range", score)
		}
	})

	t.Run("penalties exceeding 1.0", func(t *testing.T) {
		components := TDGComponents{
			Complexity:  2.0,
			Churn:       2.0,
			Coupling:    2.0,
			Duplication: 2.0,
			DomainRisk:  2.0,
		}

		score := CalculateTDG(components, weights)
		if score != 0.0 {
			t.Errorf("With excessive penalties, score = %v, expected 0.0", score)
		}
	})
}

func TestCalculateTDG_ScoreIntegration(t *testing.T) {
	weights := DefaultTDGWeights()

	testCases := []struct {
		components TDGComponents
		minScore   float64
		maxScore   float64
		severity   TDGSeverity
	}{
		{
			components: TDGComponents{0.0, 0.0, 0.0, 0.0, 0.0},
			minScore:   90.0,
			maxScore:   100.0,
			severity:   TDGExcellent,
		},
		{
			components: TDGComponents{0.3, 0.3, 0.3, 0.3, 0.3},
			minScore:   70.0,
			maxScore:   71.0,
			severity:   TDGGood,
		},
		{
			components: TDGComponents{0.5, 0.5, 0.5, 0.5, 0.5},
			minScore:   50.0,
			maxScore:   50.0,
			severity:   TDGModerate,
		},
		{
			components: TDGComponents{0.8, 0.8, 0.8, 0.8, 0.8},
			minScore:   0.0,
			maxScore:   21.0,
			severity:   TDGHighRisk,
		},
	}

	for _, tc := range testCases {
		score := CalculateTDG(tc.components, weights)
		severity := CalculateTDGSeverity(score)

		if score < tc.minScore || score > tc.maxScore {
			t.Errorf("Score %v outside expected range [%v, %v]", score, tc.minScore, tc.maxScore)
		}

		if severity != tc.severity {
			t.Errorf("Severity %v, expected %v (score: %v)", severity, tc.severity, score)
		}
	}
}
