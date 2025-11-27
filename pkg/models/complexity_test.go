package models

import (
	"testing"
)

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

// Tests ported from pmat for compatibility

func TestComplexityMetrics_Default(t *testing.T) {
	metrics := ComplexityMetrics{}
	if metrics.Cyclomatic != 0 {
		t.Errorf("Cyclomatic = %d, want 0", metrics.Cyclomatic)
	}
	if metrics.Cognitive != 0 {
		t.Errorf("Cognitive = %d, want 0", metrics.Cognitive)
	}
	if metrics.MaxNesting != 0 {
		t.Errorf("MaxNesting = %d, want 0", metrics.MaxNesting)
	}
	if metrics.Lines != 0 {
		t.Errorf("Lines = %d, want 0", metrics.Lines)
	}
}

func TestComplexityMetrics_Creation(t *testing.T) {
	metrics := ComplexityMetrics{
		Cyclomatic: 5,
		Cognitive:  10,
		MaxNesting: 3,
		Lines:      25,
	}
	if metrics.Cyclomatic != 5 {
		t.Errorf("Cyclomatic = %d, want 5", metrics.Cyclomatic)
	}
	if metrics.Cognitive != 10 {
		t.Errorf("Cognitive = %d, want 10", metrics.Cognitive)
	}
	if metrics.MaxNesting != 3 {
		t.Errorf("MaxNesting = %d, want 3", metrics.MaxNesting)
	}
	if metrics.Lines != 25 {
		t.Errorf("Lines = %d, want 25", metrics.Lines)
	}
}

func TestComplexityScore_Values(t *testing.T) {
	tests := []struct {
		name     string
		metrics  ComplexityMetrics
		expected float64
	}{
		{
			name: "zero values",
			metrics: ComplexityMetrics{
				Cyclomatic: 0,
				Cognitive:  0,
				MaxNesting: 0,
				Lines:      0,
			},
			expected: 0.0,
		},
		{
			name: "typical values",
			metrics: ComplexityMetrics{
				Cyclomatic: 10,
				Cognitive:  15,
				MaxNesting: 3,
				Lines:      50,
			},
			// Score = 10*1.0 + 15*1.2 + 3*2.0 + 50*0.1 = 10 + 18 + 6 + 5 = 39
			expected: 39.0,
		},
		{
			name: "high complexity",
			metrics: ComplexityMetrics{
				Cyclomatic: 25,
				Cognitive:  35,
				MaxNesting: 6,
				Lines:      100,
			},
			// Score = 25*1.0 + 35*1.2 + 6*2.0 + 100*0.1 = 25 + 42 + 12 + 10 = 89
			expected: 89.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.metrics.ComplexityScore()
			if got != tt.expected {
				t.Errorf("ComplexityScore() = %v, want %v", got, tt.expected)
			}
		})
	}
}

func TestFunctionComplexity_Creation(t *testing.T) {
	fn := FunctionComplexity{
		Name:      "test_function",
		StartLine: 10,
		EndLine:   25,
		Metrics: ComplexityMetrics{
			Cyclomatic: 3,
			Cognitive:  8,
		},
	}
	if fn.Name != "test_function" {
		t.Errorf("Name = %s, want test_function", fn.Name)
	}
	if fn.StartLine != 10 {
		t.Errorf("StartLine = %d, want 10", fn.StartLine)
	}
	if fn.EndLine != 25 {
		t.Errorf("EndLine = %d, want 25", fn.EndLine)
	}
	if fn.Metrics.Cyclomatic != 3 {
		t.Errorf("Metrics.Cyclomatic = %d, want 3", fn.Metrics.Cyclomatic)
	}
	if fn.Metrics.Cognitive != 8 {
		t.Errorf("Metrics.Cognitive = %d, want 8", fn.Metrics.Cognitive)
	}
}

func TestFileComplexity_Creation(t *testing.T) {
	file := FileComplexity{
		Path:            "test.go",
		TotalCyclomatic: 20,
		TotalCognitive:  35,
		Functions: []FunctionComplexity{
			{Name: "func1", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 5, Cognitive: 8}},
			{Name: "func2", StartLine: 30, EndLine: 40, Metrics: ComplexityMetrics{Cyclomatic: 7, Cognitive: 12}},
		},
	}
	if file.Path != "test.go" {
		t.Errorf("Path = %s, want test.go", file.Path)
	}
	if len(file.Functions) != 2 {
		t.Errorf("len(Functions) = %d, want 2", len(file.Functions))
	}
	if file.TotalCyclomatic != 20 {
		t.Errorf("TotalCyclomatic = %d, want 20", file.TotalCyclomatic)
	}
}

func TestComplexityHotspot_Creation(t *testing.T) {
	hotspot := ComplexityHotspot{
		File:           "test.go",
		Function:       "complex_function",
		Line:           42,
		Complexity:     25,
		ComplexityType: "cyclomatic",
	}
	if hotspot.File != "test.go" {
		t.Errorf("File = %s, want test.go", hotspot.File)
	}
	if hotspot.Function != "complex_function" {
		t.Errorf("Function = %s, want complex_function", hotspot.Function)
	}
	if hotspot.Line != 42 {
		t.Errorf("Line = %d, want 42", hotspot.Line)
	}
	if hotspot.Complexity != 25 {
		t.Errorf("Complexity = %d, want 25", hotspot.Complexity)
	}
	if hotspot.ComplexityType != "cyclomatic" {
		t.Errorf("ComplexityType = %s, want cyclomatic", hotspot.ComplexityType)
	}
}

func TestViolation_Creation(t *testing.T) {
	violation := Violation{
		Severity:  SeverityError,
		Rule:      "cyclomatic-complexity",
		Message:   "Function too complex",
		Value:     25,
		Threshold: 20,
		File:      "test.go",
		Line:      10,
		Function:  "test_func",
	}
	if violation.Severity != SeverityError {
		t.Errorf("Severity = %s, want error", violation.Severity)
	}
	if violation.Rule != "cyclomatic-complexity" {
		t.Errorf("Rule = %s, want cyclomatic-complexity", violation.Rule)
	}
	if violation.Value != 25 {
		t.Errorf("Value = %d, want 25", violation.Value)
	}
	if violation.Threshold != 20 {
		t.Errorf("Threshold = %d, want 20", violation.Threshold)
	}
}

func TestViolation_WarningSeverity(t *testing.T) {
	violation := Violation{
		Severity:  SeverityWarning,
		Rule:      "cognitive-complexity",
		Message:   "Getting complex",
		Value:     18,
		Threshold: 15,
		File:      "test.go",
		Line:      20,
		Function:  "other_func",
	}
	if violation.Severity != SeverityWarning {
		t.Errorf("Severity = %s, want warning", violation.Severity)
	}
	if violation.Value != 18 {
		t.Errorf("Value = %d, want 18", violation.Value)
	}
	if violation.Threshold != 15 {
		t.Errorf("Threshold = %d, want 15", violation.Threshold)
	}
}

func TestExtendedComplexityThresholds_Custom(t *testing.T) {
	thresholds := ExtendedComplexityThresholds{
		CyclomaticWarn:  8,
		CyclomaticError: 15,
		CognitiveWarn:   12,
		CognitiveError:  25,
		NestingMax:      4,
		MethodLength:    40,
	}
	if thresholds.CyclomaticWarn != 8 {
		t.Errorf("CyclomaticWarn = %d, want 8", thresholds.CyclomaticWarn)
	}
	if thresholds.CyclomaticError != 15 {
		t.Errorf("CyclomaticError = %d, want 15", thresholds.CyclomaticError)
	}
	if thresholds.CognitiveWarn != 12 {
		t.Errorf("CognitiveWarn = %d, want 12", thresholds.CognitiveWarn)
	}
	if thresholds.CognitiveError != 25 {
		t.Errorf("CognitiveError = %d, want 25", thresholds.CognitiveError)
	}
	if thresholds.NestingMax != 4 {
		t.Errorf("NestingMax = %d, want 4", thresholds.NestingMax)
	}
	if thresholds.MethodLength != 40 {
		t.Errorf("MethodLength = %d, want 40", thresholds.MethodLength)
	}
}
