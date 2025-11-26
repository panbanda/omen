package models

import "testing"

func TestDefaultComplexityThresholds(t *testing.T) {
	thresholds := DefaultComplexityThresholds()

	if thresholds.MaxCyclomatic != 10 {
		t.Errorf("MaxCyclomatic = %d, expected 10", thresholds.MaxCyclomatic)
	}
	if thresholds.MaxCognitive != 15 {
		t.Errorf("MaxCognitive = %d, expected 15", thresholds.MaxCognitive)
	}
	if thresholds.MaxNesting != 4 {
		t.Errorf("MaxNesting = %d, expected 4", thresholds.MaxNesting)
	}
}

func TestComplexityMetrics_IsSimple(t *testing.T) {
	thresholds := DefaultComplexityThresholds()

	tests := []struct {
		name     string
		metrics  ComplexityMetrics
		expected bool
	}{
		{
			name: "all within limits",
			metrics: ComplexityMetrics{
				Cyclomatic: 5,
				Cognitive:  10,
				MaxNesting: 2,
			},
			expected: true,
		},
		{
			name: "at limits",
			metrics: ComplexityMetrics{
				Cyclomatic: 10,
				Cognitive:  15,
				MaxNesting: 4,
			},
			expected: true,
		},
		{
			name: "cyclomatic exceeds",
			metrics: ComplexityMetrics{
				Cyclomatic: 11,
				Cognitive:  10,
				MaxNesting: 2,
			},
			expected: false,
		},
		{
			name: "cognitive exceeds",
			metrics: ComplexityMetrics{
				Cyclomatic: 5,
				Cognitive:  16,
				MaxNesting: 2,
			},
			expected: false,
		},
		{
			name: "nesting exceeds",
			metrics: ComplexityMetrics{
				Cyclomatic: 5,
				Cognitive:  10,
				MaxNesting: 5,
			},
			expected: false,
		},
		{
			name: "all exceed",
			metrics: ComplexityMetrics{
				Cyclomatic: 20,
				Cognitive:  25,
				MaxNesting: 8,
			},
			expected: false,
		},
		{
			name: "zero values",
			metrics: ComplexityMetrics{
				Cyclomatic: 0,
				Cognitive:  0,
				MaxNesting: 0,
			},
			expected: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.metrics.IsSimple(thresholds)
			if got != tt.expected {
				t.Errorf("IsSimple() = %v, expected %v", got, tt.expected)
			}
		})
	}
}

func TestComplexityMetrics_NeedsRefactoring(t *testing.T) {
	thresholds := DefaultComplexityThresholds()

	tests := []struct {
		name     string
		metrics  ComplexityMetrics
		expected bool
	}{
		{
			name: "within limits",
			metrics: ComplexityMetrics{
				Cyclomatic: 10,
				Cognitive:  15,
				MaxNesting: 4,
			},
			expected: false,
		},
		{
			name: "slightly exceeds",
			metrics: ComplexityMetrics{
				Cyclomatic: 15,
				Cognitive:  20,
				MaxNesting: 6,
			},
			expected: false,
		},
		{
			name: "cyclomatic significantly exceeds (2x)",
			metrics: ComplexityMetrics{
				Cyclomatic: 21,
				Cognitive:  10,
				MaxNesting: 2,
			},
			expected: true,
		},
		{
			name: "cognitive significantly exceeds (2x)",
			metrics: ComplexityMetrics{
				Cyclomatic: 5,
				Cognitive:  31,
				MaxNesting: 2,
			},
			expected: true,
		},
		{
			name: "nesting significantly exceeds (2x)",
			metrics: ComplexityMetrics{
				Cyclomatic: 5,
				Cognitive:  10,
				MaxNesting: 9,
			},
			expected: true,
		},
		{
			name: "all significantly exceed",
			metrics: ComplexityMetrics{
				Cyclomatic: 50,
				Cognitive:  60,
				MaxNesting: 20,
			},
			expected: true,
		},
		{
			name: "exactly at 2x threshold",
			metrics: ComplexityMetrics{
				Cyclomatic: 20,
				Cognitive:  30,
				MaxNesting: 8,
			},
			expected: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.metrics.NeedsRefactoring(thresholds)
			if got != tt.expected {
				t.Errorf("NeedsRefactoring() = %v, expected %v", got, tt.expected)
			}
		})
	}
}

func TestComplexityMetrics_CustomThresholds(t *testing.T) {
	customThresholds := ComplexityThresholds{
		MaxCyclomatic: 5,
		MaxCognitive:  8,
		MaxNesting:    2,
	}

	metrics := ComplexityMetrics{
		Cyclomatic: 6,
		Cognitive:  7,
		MaxNesting: 2,
	}

	if metrics.IsSimple(customThresholds) {
		t.Error("Should not be simple with custom thresholds")
	}

	if metrics.NeedsRefactoring(customThresholds) {
		t.Error("Should not need refactoring (not 2x threshold)")
	}
}

func TestComplexityMetrics_EdgeCases(t *testing.T) {
	t.Run("zero thresholds", func(t *testing.T) {
		zeroThresholds := ComplexityThresholds{
			MaxCyclomatic: 0,
			MaxCognitive:  0,
			MaxNesting:    0,
		}

		metrics := ComplexityMetrics{
			Cyclomatic: 1,
			Cognitive:  1,
			MaxNesting: 1,
		}

		if metrics.IsSimple(zeroThresholds) {
			t.Error("Should not be simple with zero thresholds")
		}

		if !metrics.NeedsRefactoring(zeroThresholds) {
			t.Error("Should need refactoring with zero thresholds")
		}
	})

	t.Run("max values", func(t *testing.T) {
		maxThresholds := ComplexityThresholds{
			MaxCyclomatic: 1000,
			MaxCognitive:  1000,
			MaxNesting:    100,
		}

		metrics := ComplexityMetrics{
			Cyclomatic: 999,
			Cognitive:  999,
			MaxNesting: 99,
		}

		if !metrics.IsSimple(maxThresholds) {
			t.Error("Should be simple with very high thresholds")
		}
	})
}
