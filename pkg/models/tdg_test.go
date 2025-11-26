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
	if weights.Churn != 0.35 {
		t.Errorf("Churn = %v, expected 0.35", weights.Churn)
	}
	if weights.Coupling != 0.15 {
		t.Errorf("Coupling = %v, expected 0.15", weights.Coupling)
	}
	if weights.Duplication != 0.10 {
		t.Errorf("Duplication = %v, expected 0.10", weights.Duplication)
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
			name:     "critical - 4.0",
			score:    4.0,
			expected: TDGCritical,
		},
		{
			name:     "critical - just above 2.5",
			score:    2.51,
			expected: TDGCritical,
		},
		{
			name:     "warning - 2.0",
			score:    2.0,
			expected: TDGWarning,
		},
		{
			name:     "warning - at 1.5",
			score:    1.5,
			expected: TDGWarning,
		},
		{
			name:     "normal - 1.0",
			score:    1.0,
			expected: TDGNormal,
		},
		{
			name:     "normal - 0",
			score:    0.0,
			expected: TDGNormal,
		},
		{
			name:     "normal - just below 1.5",
			score:    1.49,
			expected: TDGNormal,
		},
		{
			name:     "warning - at boundary",
			score:    2.5,
			expected: TDGWarning,
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
			name: "no debt - zero score",
			components: TDGComponents{
				Complexity:  0.0,
				Churn:       0.0,
				Coupling:    0.0,
				Duplication: 0.0,
				DomainRisk:  0.0,
			},
			expected:  0.0,
			tolerance: 0.01,
		},
		{
			name: "maximum debt",
			components: TDGComponents{
				Complexity:  5.0,
				Churn:       5.0,
				Coupling:    5.0,
				Duplication: 5.0,
				DomainRisk:  5.0,
			},
			expected:  5.0,
			tolerance: 0.01,
		},
		{
			name: "moderate debt",
			components: TDGComponents{
				Complexity:  2.5,
				Churn:       2.5,
				Coupling:    2.5,
				Duplication: 2.5,
				DomainRisk:  2.5,
			},
			expected:  2.5,
			tolerance: 0.01,
		},
		{
			name: "high complexity only",
			components: TDGComponents{
				Complexity:  5.0,
				Churn:       0.0,
				Coupling:    0.0,
				Duplication: 0.0,
				DomainRisk:  0.0,
			},
			expected:  1.5, // 5.0 * 0.30
			tolerance: 0.01,
		},
		{
			name: "mixed components",
			components: TDGComponents{
				Complexity:  4.0,
				Churn:       1.5,
				Coupling:    1.0,
				Duplication: 2.5,
				DomainRisk:  0.5,
			},
			expected:  4.0*0.30 + 1.5*0.35 + 1.0*0.15 + 2.5*0.10 + 0.5*0.10,
			tolerance: 0.01,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateTDG(tt.components, weights)

			if math.Abs(got-tt.expected) > tt.tolerance {
				t.Errorf("CalculateTDG() = %v, expected %v (±%v)", got, tt.expected, tt.tolerance)
			}

			if got < 0.0 || got > 5.0 {
				t.Errorf("TDG score %v is outside valid range [0.0, 5.0]", got)
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
		Complexity:  5.0,
		Churn:       0.0,
		Coupling:    0.0,
		Duplication: 0.0,
		DomainRisk:  0.0,
	}

	score := CalculateTDG(components, customWeights)
	expected := 3.0 // 5.0 * 0.60

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
			name:     "normal",
			severity: TDGNormal,
			expected: "\033[32m",
		},
		{
			name:     "warning",
			severity: TDGWarning,
			expected: "\033[33m",
		},
		{
			name:     "critical",
			severity: TDGCritical,
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
	if TDGNormal != "normal" {
		t.Errorf("TDGNormal = %s, expected normal", TDGNormal)
	}
	if TDGWarning != "warning" {
		t.Errorf("TDGWarning = %s, expected warning", TDGWarning)
	}
	if TDGCritical != "critical" {
		t.Errorf("TDGCritical = %s, expected critical", TDGCritical)
	}
}

func TestCalculateTDG_BoundaryConditions(t *testing.T) {
	weights := DefaultTDGWeights()

	t.Run("negative components clamped to 0", func(t *testing.T) {
		components := TDGComponents{
			Complexity:  -0.5,
			Churn:       0.0,
			Coupling:    0.0,
			Duplication: 0.0,
			DomainRisk:  0.0,
		}

		score := CalculateTDG(components, weights)
		if score < 0 || score > 5 {
			t.Errorf("Score %v outside valid range", score)
		}
	})

	t.Run("components exceeding 5.0", func(t *testing.T) {
		components := TDGComponents{
			Complexity:  10.0,
			Churn:       10.0,
			Coupling:    10.0,
			Duplication: 10.0,
			DomainRisk:  10.0,
		}

		score := CalculateTDG(components, weights)
		if score != 5.0 {
			t.Errorf("With excessive components, score = %v, expected 5.0", score)
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
			minScore:   0.0,
			maxScore:   0.0,
			severity:   TDGNormal,
		},
		{
			components: TDGComponents{1.5, 1.5, 1.5, 1.5, 1.5},
			minScore:   1.4,
			maxScore:   1.6,
			severity:   TDGNormal,
		},
		{
			components: TDGComponents{2.5, 2.5, 2.5, 2.5, 2.5},
			minScore:   2.4,
			maxScore:   2.6,
			severity:   TDGWarning,
		},
		{
			components: TDGComponents{4.0, 4.0, 4.0, 4.0, 4.0},
			minScore:   3.9,
			maxScore:   4.1,
			severity:   TDGCritical,
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
