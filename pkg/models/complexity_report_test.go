package models

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestAggregateResults_Empty(t *testing.T) {
	report := AggregateResults(nil)

	if report.Summary.TotalFiles != 0 {
		t.Errorf("TotalFiles = %d, want 0", report.Summary.TotalFiles)
	}
	if report.Summary.TotalFunctions != 0 {
		t.Errorf("TotalFunctions = %d, want 0", report.Summary.TotalFunctions)
	}
	if len(report.Violations) != 0 {
		t.Errorf("len(Violations) = %d, want 0", len(report.Violations))
	}
	if len(report.Hotspots) != 0 {
		t.Errorf("len(Hotspots) = %d, want 0", len(report.Hotspots))
	}
}

func TestAggregateResults_SingleFile(t *testing.T) {
	files := []FileComplexity{
		{
			Path:            "test.go",
			TotalCyclomatic: 10,
			TotalCognitive:  15,
			Functions: []FunctionComplexity{
				{
					Name:      "func1",
					StartLine: 10,
					EndLine:   20,
					Metrics:   ComplexityMetrics{Cyclomatic: 5, Cognitive: 8, MaxNesting: 2, Lines: 10},
				},
				{
					Name:      "func2",
					StartLine: 30,
					EndLine:   50,
					Metrics:   ComplexityMetrics{Cyclomatic: 15, Cognitive: 20, MaxNesting: 3, Lines: 20}, // Should trigger warning
				},
			},
		},
	}

	report := AggregateResults(files)

	if report.Summary.TotalFiles != 1 {
		t.Errorf("TotalFiles = %d, want 1", report.Summary.TotalFiles)
	}
	if report.Summary.TotalFunctions != 2 {
		t.Errorf("TotalFunctions = %d, want 2", report.Summary.TotalFunctions)
	}
	if report.Summary.MaxCyclomatic != 15 {
		t.Errorf("MaxCyclomatic = %d, want 15", report.Summary.MaxCyclomatic)
	}
	if report.Summary.MaxCognitive != 20 {
		t.Errorf("MaxCognitive = %d, want 20", report.Summary.MaxCognitive)
	}
	// func2 should have violations (15 > 10 warn for cyclomatic, 20 > 15 warn for cognitive)
	if len(report.Violations) < 2 {
		t.Errorf("len(Violations) = %d, want >= 2", len(report.Violations))
	}
	// func2 should be a hotspot
	if len(report.Hotspots) < 1 {
		t.Errorf("len(Hotspots) = %d, want >= 1", len(report.Hotspots))
	}
}

func TestAggregateResultsWithThresholds(t *testing.T) {
	files := []FileComplexity{
		{
			Path: "test.go",
			Functions: []FunctionComplexity{
				{
					Name:      "func1",
					StartLine: 10,
					EndLine:   20,
					Metrics:   ComplexityMetrics{Cyclomatic: 25, Cognitive: 30, MaxNesting: 4, Lines: 50},
				},
			},
		},
	}

	// With default thresholds, func1 should be an error (25 > 20, 30 > 30)
	reportDefault := AggregateResults(files)
	errorCountDefault := reportDefault.ErrorCount()

	// With higher thresholds, should be fewer errors
	maxCyc := uint32(35)
	maxCog := uint32(35)
	reportCustom := AggregateResultsWithThresholds(files, &maxCyc, &maxCog)
	errorCountCustom := reportCustom.ErrorCount()

	if errorCountCustom >= errorCountDefault {
		t.Errorf("Custom thresholds should reduce errors: default=%d, custom=%d", errorCountDefault, errorCountCustom)
	}
}

func TestCalculateMedian(t *testing.T) {
	tests := []struct {
		name   string
		values []uint32
		want   float32
	}{
		{"empty", nil, 0},
		{"single", []uint32{5}, 5},
		{"two_even", []uint32{2, 8}, 5},
		{"three_odd", []uint32{1, 5, 10}, 5},
		{"four_even", []uint32{1, 2, 8, 10}, 5},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := calculateMedian(tt.values)
			if got != tt.want {
				t.Errorf("calculateMedian() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestCalculateTechnicalDebt(t *testing.T) {
	violations := []Violation{
		{Severity: SeverityWarning, Value: 15, Threshold: 10}, // 5 points over = 5*15 = 75 min
		{Severity: SeverityError, Value: 25, Threshold: 20},   // 5 points over = 5*30 = 150 min
	}

	debt := calculateTechnicalDebt(violations)
	expected := float32(225.0 / 60.0) // 3.75 hours

	if debt != expected {
		t.Errorf("calculateTechnicalDebt() = %v, want %v", debt, expected)
	}
}

func TestComplexityScore(t *testing.T) {
	metrics := ComplexityMetrics{
		Cyclomatic: 10,
		Cognitive:  15,
		MaxNesting: 3,
		Lines:      50,
	}

	// Score = 10*1.0 + 15*1.2 + 3*2.0 + 50*0.1 = 10 + 18 + 6 + 5 = 39
	expected := 39.0
	got := metrics.ComplexityScore()

	if got != expected {
		t.Errorf("ComplexityScore() = %v, want %v", got, expected)
	}
}

func TestIsSimpleDefault(t *testing.T) {
	tests := []struct {
		name       string
		cyclomatic uint32
		cognitive  uint32
		want       bool
	}{
		{"simple", 3, 5, true},
		{"at_limit", 5, 7, true},
		{"cyclomatic_over", 6, 5, false},
		{"cognitive_over", 3, 8, false},
		{"both_over", 10, 15, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			m := ComplexityMetrics{Cyclomatic: tt.cyclomatic, Cognitive: tt.cognitive}
			if got := m.IsSimpleDefault(); got != tt.want {
				t.Errorf("IsSimpleDefault() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestNeedsRefactoringDefault(t *testing.T) {
	tests := []struct {
		name       string
		cyclomatic uint32
		cognitive  uint32
		want       bool
	}{
		{"simple", 3, 5, false},
		{"at_limit", 10, 15, false},
		{"cyclomatic_over", 11, 10, true},
		{"cognitive_over", 5, 16, true},
		{"both_over", 15, 20, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			m := ComplexityMetrics{Cyclomatic: tt.cyclomatic, Cognitive: tt.cognitive}
			if got := m.NeedsRefactoringDefault(); got != tt.want {
				t.Errorf("NeedsRefactoringDefault() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestDefaultExtendedThresholds(t *testing.T) {
	thresholds := DefaultExtendedThresholds()

	if thresholds.CyclomaticWarn != 10 {
		t.Errorf("CyclomaticWarn = %d, want 10", thresholds.CyclomaticWarn)
	}
	if thresholds.CyclomaticError != 20 {
		t.Errorf("CyclomaticError = %d, want 20", thresholds.CyclomaticError)
	}
	if thresholds.CognitiveWarn != 15 {
		t.Errorf("CognitiveWarn = %d, want 15", thresholds.CognitiveWarn)
	}
	if thresholds.CognitiveError != 30 {
		t.Errorf("CognitiveError = %d, want 30", thresholds.CognitiveError)
	}
	if thresholds.NestingMax != 5 {
		t.Errorf("NestingMax = %d, want 5", thresholds.NestingMax)
	}
	if thresholds.MethodLength != 50 {
		t.Errorf("MethodLength = %d, want 50", thresholds.MethodLength)
	}
}

func TestComplexityReport_ErrorWarningCount(t *testing.T) {
	report := &ComplexityReport{
		Violations: []Violation{
			{Severity: SeverityError},
			{Severity: SeverityError},
			{Severity: SeverityWarning},
			{Severity: SeverityWarning},
			{Severity: SeverityWarning},
		},
	}

	if report.ErrorCount() != 2 {
		t.Errorf("ErrorCount() = %d, want 2", report.ErrorCount())
	}
	if report.WarningCount() != 3 {
		t.Errorf("WarningCount() = %d, want 3", report.WarningCount())
	}
}

// Additional tests ported from pmat for compatibility

func TestAggregateResults_MedianCalculationOdd(t *testing.T) {
	files := []FileComplexity{
		{
			Path: "test.go",
			Functions: []FunctionComplexity{
				{Name: "func1", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 5, Cognitive: 10}},
				{Name: "func2", StartLine: 30, EndLine: 40, Metrics: ComplexityMetrics{Cyclomatic: 7, Cognitive: 12}},
				{Name: "func3", StartLine: 50, EndLine: 60, Metrics: ComplexityMetrics{Cyclomatic: 9, Cognitive: 15}},
			},
		},
	}

	report := AggregateResults(files)

	// With values [5, 7, 9], median should be 7
	if report.Summary.MedianCyclomatic != 7.0 {
		t.Errorf("MedianCyclomatic = %v, want 7.0", report.Summary.MedianCyclomatic)
	}
	// With values [10, 12, 15], median should be 12
	if report.Summary.MedianCognitive != 12.0 {
		t.Errorf("MedianCognitive = %v, want 12.0", report.Summary.MedianCognitive)
	}
}

func TestAggregateResults_PercentileCalculation(t *testing.T) {
	var functions []FunctionComplexity
	for i := 1; i <= 10; i++ {
		functions = append(functions, FunctionComplexity{
			Name:      "func" + string(rune('0'+i)),
			StartLine: uint32(i * 10),
			EndLine:   uint32(i*10 + 10),
			Metrics:   ComplexityMetrics{Cyclomatic: uint32(i), Cognitive: uint32(i * 2)},
		})
	}

	files := []FileComplexity{
		{
			Path:      "test.go",
			Functions: functions,
		},
	}

	report := AggregateResults(files)

	// p90 of [1,2,3,4,5,6,7,8,9,10] should be around 9 or 10
	if report.Summary.P90Cyclomatic < 9 || report.Summary.P90Cyclomatic > 10 {
		t.Errorf("P90Cyclomatic = %d, want 9 or 10", report.Summary.P90Cyclomatic)
	}
	// p90 of [2,4,6,8,10,12,14,16,18,20] should be around 18 or 20
	if report.Summary.P90Cognitive < 18 || report.Summary.P90Cognitive > 20 {
		t.Errorf("P90Cognitive = %d, want 18-20", report.Summary.P90Cognitive)
	}
}

func TestAggregateResults_TechnicalDebtCalculation(t *testing.T) {
	files := []FileComplexity{
		{
			Path: "test.go",
			Functions: []FunctionComplexity{
				// Warning: 5 over cyc warn (10), 5 over cog warn (15)
				{Name: "func1", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 15, Cognitive: 20}},
				// Error: 5 over cyc error (20), 5 over cog error (30)
				{Name: "func2", StartLine: 30, EndLine: 40, Metrics: ComplexityMetrics{Cyclomatic: 25, Cognitive: 35}},
			},
		},
	}

	report := AggregateResults(files)

	// Should have violations and technical debt
	if len(report.Violations) == 0 {
		t.Error("Expected violations")
	}
	if report.Summary.TechnicalDebtHours <= 0 {
		t.Error("Expected positive technical debt")
	}
}

func TestAggregateResults_HotspotSorting(t *testing.T) {
	files := []FileComplexity{
		{
			Path: "test.go",
			Functions: []FunctionComplexity{
				{Name: "low_complexity", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 12, Cognitive: 18}},
				{Name: "high_complexity", StartLine: 30, EndLine: 40, Metrics: ComplexityMetrics{Cyclomatic: 25, Cognitive: 35}},
				{Name: "medium_complexity", StartLine: 50, EndLine: 60, Metrics: ComplexityMetrics{Cyclomatic: 15, Cognitive: 22}},
			},
		},
	}

	report := AggregateResults(files)

	// Hotspots should be sorted by complexity (descending)
	if len(report.Hotspots) < 3 {
		t.Fatalf("Expected at least 3 hotspots, got %d", len(report.Hotspots))
	}
	if report.Hotspots[0].Function != "high_complexity" {
		t.Errorf("First hotspot = %s, want high_complexity", report.Hotspots[0].Function)
	}
	if report.Hotspots[0].Complexity != 25 {
		t.Errorf("First hotspot complexity = %d, want 25", report.Hotspots[0].Complexity)
	}
}

func TestAggregateResults_MultipleFiles(t *testing.T) {
	files := []FileComplexity{
		{
			Path: "file1.go",
			Functions: []FunctionComplexity{
				{Name: "func1", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 5, Cognitive: 8}},
			},
		},
		{
			Path: "file2.go",
			Functions: []FunctionComplexity{
				{Name: "func2", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 7, Cognitive: 10}},
				{Name: "func3", StartLine: 30, EndLine: 40, Metrics: ComplexityMetrics{Cyclomatic: 3, Cognitive: 5}},
			},
		},
	}

	report := AggregateResults(files)

	if report.Summary.TotalFiles != 2 {
		t.Errorf("TotalFiles = %d, want 2", report.Summary.TotalFiles)
	}
	if report.Summary.TotalFunctions != 3 {
		t.Errorf("TotalFunctions = %d, want 3", report.Summary.TotalFunctions)
	}
	if report.Summary.MaxCyclomatic != 7 {
		t.Errorf("MaxCyclomatic = %d, want 7", report.Summary.MaxCyclomatic)
	}
	if report.Summary.MaxCognitive != 10 {
		t.Errorf("MaxCognitive = %d, want 10", report.Summary.MaxCognitive)
	}
}

func TestMaxValue(t *testing.T) {
	tests := []struct {
		name   string
		values []uint32
		want   uint32
	}{
		{"empty", nil, 0},
		{"single", []uint32{5}, 5},
		{"multiple", []uint32{1, 5, 3}, 5},
		{"all_same", []uint32{7, 7, 7}, 7},
		{"descending", []uint32{10, 5, 1}, 10},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := maxValue(tt.values)
			if got != tt.want {
				t.Errorf("maxValue() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestPercentileU32(t *testing.T) {
	tests := []struct {
		name    string
		values  []uint32
		p       int
		wantMin uint32
		wantMax uint32
	}{
		{"empty", nil, 90, 0, 0},
		{"single", []uint32{5}, 90, 5, 5},
		{"ten_values_p90", []uint32{1, 2, 3, 4, 5, 6, 7, 8, 9, 10}, 90, 9, 10},
		{"ten_values_p50", []uint32{1, 2, 3, 4, 5, 6, 7, 8, 9, 10}, 50, 5, 6},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := percentileU32(tt.values, tt.p)
			if got < tt.wantMin || got > tt.wantMax {
				t.Errorf("percentileU32() = %d, want between %d and %d", got, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestBuildCustomThresholds(t *testing.T) {
	t.Run("nil values use defaults", func(t *testing.T) {
		thresholds := buildCustomThresholds(nil, nil)
		if thresholds.CyclomaticError != 20 {
			t.Errorf("CyclomaticError = %d, want 20", thresholds.CyclomaticError)
		}
		if thresholds.CognitiveError != 30 {
			t.Errorf("CognitiveError = %d, want 30", thresholds.CognitiveError)
		}
	})

	t.Run("custom cyclomatic", func(t *testing.T) {
		maxCyc := uint32(15)
		thresholds := buildCustomThresholds(&maxCyc, nil)
		if thresholds.CyclomaticError != 15 {
			t.Errorf("CyclomaticError = %d, want 15", thresholds.CyclomaticError)
		}
		if thresholds.CyclomaticWarn != 10 {
			t.Errorf("CyclomaticWarn = %d, want 10", thresholds.CyclomaticWarn)
		}
	})

	t.Run("custom cognitive", func(t *testing.T) {
		maxCog := uint32(25)
		thresholds := buildCustomThresholds(nil, &maxCog)
		if thresholds.CognitiveError != 25 {
			t.Errorf("CognitiveError = %d, want 25", thresholds.CognitiveError)
		}
		if thresholds.CognitiveWarn != 20 {
			t.Errorf("CognitiveWarn = %d, want 20", thresholds.CognitiveWarn)
		}
	})

	t.Run("very low threshold", func(t *testing.T) {
		maxCyc := uint32(3)
		thresholds := buildCustomThresholds(&maxCyc, nil)
		if thresholds.CyclomaticError != 3 {
			t.Errorf("CyclomaticError = %d, want 3", thresholds.CyclomaticError)
		}
		if thresholds.CyclomaticWarn != 1 {
			t.Errorf("CyclomaticWarn = %d, want 1 (min when threshold <= 5)", thresholds.CyclomaticWarn)
		}
	})
}

// Tests for violation checking logic (ported from pmat)

func TestCheckFunctionViolations_NoViolation(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "simple_func",
		StartLine: 10,
		Metrics:   ComplexityMetrics{Cyclomatic: 5, Cognitive: 10}, // Below warn thresholds
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 0 {
		t.Errorf("Expected no violations, got %d", len(violations))
	}
}

func TestCheckFunctionViolations_CyclomaticWarning(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "warning_func",
		StartLine: 10,
		Metrics:   ComplexityMetrics{Cyclomatic: 15, Cognitive: 10}, // Above warn (10), below error (20)
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 1 {
		t.Fatalf("Expected 1 violation, got %d", len(violations))
	}
	if violations[0].Severity != SeverityWarning {
		t.Errorf("Severity = %s, want warning", violations[0].Severity)
	}
	if violations[0].Rule != "cyclomatic-complexity" {
		t.Errorf("Rule = %s, want cyclomatic-complexity", violations[0].Rule)
	}
	if violations[0].Value != 15 {
		t.Errorf("Value = %d, want 15", violations[0].Value)
	}
	if violations[0].Threshold != 10 {
		t.Errorf("Threshold = %d, want 10", violations[0].Threshold)
	}
	if violations[0].File != "test.go" {
		t.Errorf("File = %s, want test.go", violations[0].File)
	}
	if violations[0].Function != "warning_func" {
		t.Errorf("Function = %s, want warning_func", violations[0].Function)
	}
}

func TestCheckFunctionViolations_CyclomaticError(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "error_func",
		StartLine: 10,
		Metrics:   ComplexityMetrics{Cyclomatic: 25, Cognitive: 10}, // Above error threshold (20)
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 1 {
		t.Fatalf("Expected 1 violation, got %d", len(violations))
	}
	if violations[0].Severity != SeverityError {
		t.Errorf("Severity = %s, want error", violations[0].Severity)
	}
	if violations[0].Rule != "cyclomatic-complexity" {
		t.Errorf("Rule = %s, want cyclomatic-complexity", violations[0].Rule)
	}
	if violations[0].Value != 25 {
		t.Errorf("Value = %d, want 25", violations[0].Value)
	}
	if violations[0].Threshold != 20 {
		t.Errorf("Threshold = %d, want 20", violations[0].Threshold)
	}
}

func TestCheckFunctionViolations_CognitiveWarning(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "cognitive_warning",
		StartLine: 20,
		Metrics:   ComplexityMetrics{Cyclomatic: 5, Cognitive: 20}, // Above warn (15), below error (30)
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 1 {
		t.Fatalf("Expected 1 violation, got %d", len(violations))
	}
	if violations[0].Severity != SeverityWarning {
		t.Errorf("Severity = %s, want warning", violations[0].Severity)
	}
	if violations[0].Rule != "cognitive-complexity" {
		t.Errorf("Rule = %s, want cognitive-complexity", violations[0].Rule)
	}
	if violations[0].Value != 20 {
		t.Errorf("Value = %d, want 20", violations[0].Value)
	}
	if violations[0].Threshold != 15 {
		t.Errorf("Threshold = %d, want 15", violations[0].Threshold)
	}
}

func TestCheckFunctionViolations_CognitiveError(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "cognitive_error",
		StartLine: 20,
		Metrics:   ComplexityMetrics{Cyclomatic: 5, Cognitive: 35}, // Above error threshold (30)
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 1 {
		t.Fatalf("Expected 1 violation, got %d", len(violations))
	}
	if violations[0].Severity != SeverityError {
		t.Errorf("Severity = %s, want error", violations[0].Severity)
	}
	if violations[0].Rule != "cognitive-complexity" {
		t.Errorf("Rule = %s, want cognitive-complexity", violations[0].Rule)
	}
	if violations[0].Value != 35 {
		t.Errorf("Value = %d, want 35", violations[0].Value)
	}
	if violations[0].Threshold != 30 {
		t.Errorf("Threshold = %d, want 30", violations[0].Threshold)
	}
}

func TestCheckFunctionViolations_BothExceed(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "both_exceed",
		StartLine: 30,
		Metrics:   ComplexityMetrics{Cyclomatic: 25, Cognitive: 35}, // Both exceed error thresholds
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 2 {
		t.Fatalf("Expected 2 violations, got %d", len(violations))
	}

	// Check we have both cyclomatic and cognitive violations
	hasCC := false
	hasCog := false
	for _, v := range violations {
		if v.Rule == "cyclomatic-complexity" {
			hasCC = true
			if v.Severity != SeverityError {
				t.Errorf("Cyclomatic violation should be error")
			}
		}
		if v.Rule == "cognitive-complexity" {
			hasCog = true
			if v.Severity != SeverityError {
				t.Errorf("Cognitive violation should be error")
			}
		}
	}
	if !hasCC {
		t.Error("Missing cyclomatic-complexity violation")
	}
	if !hasCog {
		t.Error("Missing cognitive-complexity violation")
	}
}

func TestCheckFunctionHotspots(t *testing.T) {
	thresholds := DefaultExtendedThresholds()

	t.Run("below threshold - no hotspot", func(t *testing.T) {
		fn := &FunctionComplexity{
			Name:      "simple",
			StartLine: 10,
			Metrics:   ComplexityMetrics{Cyclomatic: 5},
		}
		var hotspots []ComplexityHotspot
		checkFunctionHotspots(fn, "test.go", thresholds, &hotspots)

		if len(hotspots) != 0 {
			t.Errorf("Expected no hotspots, got %d", len(hotspots))
		}
	})

	t.Run("above threshold - is hotspot", func(t *testing.T) {
		fn := &FunctionComplexity{
			Name:      "complex",
			StartLine: 20,
			Metrics:   ComplexityMetrics{Cyclomatic: 15},
		}
		var hotspots []ComplexityHotspot
		checkFunctionHotspots(fn, "test.go", thresholds, &hotspots)

		if len(hotspots) != 1 {
			t.Fatalf("Expected 1 hotspot, got %d", len(hotspots))
		}
		if hotspots[0].File != "test.go" {
			t.Errorf("File = %s, want test.go", hotspots[0].File)
		}
		if hotspots[0].Function != "complex" {
			t.Errorf("Function = %s, want complex", hotspots[0].Function)
		}
		if hotspots[0].Line != 20 {
			t.Errorf("Line = %d, want 20", hotspots[0].Line)
		}
		if hotspots[0].Complexity != 15 {
			t.Errorf("Complexity = %d, want 15", hotspots[0].Complexity)
		}
		if hotspots[0].ComplexityType != "cyclomatic" {
			t.Errorf("ComplexityType = %s, want cyclomatic", hotspots[0].ComplexityType)
		}
	})
}

func TestAggregateResults_HotspotLimit(t *testing.T) {
	// Create 15 functions that all exceed thresholds
	var functions []FunctionComplexity
	for i := 1; i <= 15; i++ {
		functions = append(functions, FunctionComplexity{
			Name:      "func" + string(rune('A'+i-1)),
			StartLine: uint32(i * 10),
			EndLine:   uint32(i*10 + 10),
			Metrics:   ComplexityMetrics{Cyclomatic: uint32(10 + i), Cognitive: uint32(15 + i)},
		})
	}

	files := []FileComplexity{
		{Path: "test.go", Functions: functions},
	}

	report := AggregateResults(files)

	// Should be limited to 10 hotspots
	if len(report.Hotspots) > 10 {
		t.Errorf("Hotspots should be limited to 10, got %d", len(report.Hotspots))
	}

	// Hotspots should be sorted by complexity (highest first)
	for i := 1; i < len(report.Hotspots); i++ {
		if report.Hotspots[i].Complexity > report.Hotspots[i-1].Complexity {
			t.Error("Hotspots should be sorted by complexity descending")
		}
	}
}

func TestViolation_MessageContents(t *testing.T) {
	thresholds := DefaultExtendedThresholds()
	fn := &FunctionComplexity{
		Name:      "test_func",
		StartLine: 42,
		Metrics:   ComplexityMetrics{Cyclomatic: 15, Cognitive: 10},
	}

	var violations []Violation
	checkFunctionViolations(fn, "test.go", thresholds, &violations)

	if len(violations) != 1 {
		t.Fatalf("Expected 1 violation, got %d", len(violations))
	}

	// Message should contain both the value and threshold
	msg := violations[0].Message
	if msg == "" {
		t.Error("Message should not be empty")
	}
	// Should mention the actual value and the threshold
	if violations[0].Value == 0 {
		t.Error("Value should be set")
	}
	if violations[0].Threshold == 0 {
		t.Error("Threshold should be set")
	}
}

// JSON serialization tests (ported from pmat)

func TestViolation_JSONSerialization(t *testing.T) {
	errorViolation := Violation{
		Severity:  SeverityError,
		Rule:      "cyclomatic-complexity",
		Message:   "Function too complex",
		Value:     25,
		Threshold: 20,
		File:      "test.go",
		Line:      10,
		Function:  "test_func",
	}

	warningViolation := Violation{
		Severity:  SeverityWarning,
		Rule:      "cognitive-complexity",
		Message:   "Getting complex",
		Value:     18,
		Threshold: 15,
		File:      "test.go",
		Line:      20,
		Function:  "other_func",
	}

	// Test error violation serialization
	errorJSON, err := json.Marshal(errorViolation)
	if err != nil {
		t.Fatalf("Failed to marshal error violation: %v", err)
	}
	if !strings.Contains(string(errorJSON), `"severity":"error"`) {
		t.Error("Error violation JSON should contain severity:error")
	}
	if !strings.Contains(string(errorJSON), `"rule":"cyclomatic-complexity"`) {
		t.Error("Error violation JSON should contain rule")
	}

	// Test warning violation serialization
	warningJSON, err := json.Marshal(warningViolation)
	if err != nil {
		t.Fatalf("Failed to marshal warning violation: %v", err)
	}
	if !strings.Contains(string(warningJSON), `"severity":"warning"`) {
		t.Error("Warning violation JSON should contain severity:warning")
	}

	// Test deserialization
	var deserializedError Violation
	if err := json.Unmarshal(errorJSON, &deserializedError); err != nil {
		t.Fatalf("Failed to unmarshal error violation: %v", err)
	}
	if deserializedError.Severity != SeverityError {
		t.Errorf("Deserialized severity = %s, want error", deserializedError.Severity)
	}
	if deserializedError.Value != 25 {
		t.Errorf("Deserialized value = %d, want 25", deserializedError.Value)
	}

	var deserializedWarning Violation
	if err := json.Unmarshal(warningJSON, &deserializedWarning); err != nil {
		t.Fatalf("Failed to unmarshal warning violation: %v", err)
	}
	if deserializedWarning.Severity != SeverityWarning {
		t.Errorf("Deserialized severity = %s, want warning", deserializedWarning.Severity)
	}
}

func TestComplexityReport_JSONSerialization(t *testing.T) {
	report := &ComplexityReport{
		Summary: ExtendedComplexitySummary{
			TotalFiles:         2,
			TotalFunctions:     5,
			MedianCyclomatic:   8.5,
			MedianCognitive:    12.3,
			MaxCyclomatic:      25,
			MaxCognitive:       30,
			P90Cyclomatic:      20,
			P90Cognitive:       25,
			TechnicalDebtHours: 2.5,
		},
		Violations: []Violation{
			{Severity: SeverityError, Rule: "cyclomatic-complexity", Value: 25, Threshold: 20},
		},
		Hotspots: []ComplexityHotspot{
			{File: "test.go", Function: "complex_func", Line: 42, Complexity: 25, ComplexityType: "cyclomatic"},
		},
		TechnicalDebtHours: 2.5,
	}

	data, err := json.Marshal(report)
	if err != nil {
		t.Fatalf("Failed to marshal report: %v", err)
	}

	// Verify JSON contains expected fields
	jsonStr := string(data)
	if !strings.Contains(jsonStr, `"total_files":2`) {
		t.Error("JSON should contain total_files")
	}
	if !strings.Contains(jsonStr, `"total_functions":5`) {
		t.Error("JSON should contain total_functions")
	}
	if !strings.Contains(jsonStr, `"violations"`) {
		t.Error("JSON should contain violations")
	}
	if !strings.Contains(jsonStr, `"hotspots"`) {
		t.Error("JSON should contain hotspots")
	}

	// Test deserialization
	var deserialized ComplexityReport
	if err := json.Unmarshal(data, &deserialized); err != nil {
		t.Fatalf("Failed to unmarshal report: %v", err)
	}
	if deserialized.Summary.TotalFiles != 2 {
		t.Errorf("TotalFiles = %d, want 2", deserialized.Summary.TotalFiles)
	}
	if len(deserialized.Violations) != 1 {
		t.Errorf("len(Violations) = %d, want 1", len(deserialized.Violations))
	}
	if len(deserialized.Hotspots) != 1 {
		t.Errorf("len(Hotspots) = %d, want 1", len(deserialized.Hotspots))
	}
}

func TestComplexityMetrics_JSONSerialization(t *testing.T) {
	metrics := ComplexityMetrics{
		Cyclomatic: 10,
		Cognitive:  15,
		MaxNesting: 3,
		Lines:      50,
	}

	data, err := json.Marshal(metrics)
	if err != nil {
		t.Fatalf("Failed to marshal metrics: %v", err)
	}

	jsonStr := string(data)
	if !strings.Contains(jsonStr, `"cyclomatic":10`) {
		t.Error("JSON should contain cyclomatic")
	}
	if !strings.Contains(jsonStr, `"cognitive":15`) {
		t.Error("JSON should contain cognitive")
	}
	if !strings.Contains(jsonStr, `"max_nesting":3`) {
		t.Error("JSON should contain max_nesting")
	}
	if !strings.Contains(jsonStr, `"lines":50`) {
		t.Error("JSON should contain lines")
	}

	var deserialized ComplexityMetrics
	if err := json.Unmarshal(data, &deserialized); err != nil {
		t.Fatalf("Failed to unmarshal metrics: %v", err)
	}
	if deserialized.Cyclomatic != 10 {
		t.Errorf("Cyclomatic = %d, want 10", deserialized.Cyclomatic)
	}
	if deserialized.Cognitive != 15 {
		t.Errorf("Cognitive = %d, want 15", deserialized.Cognitive)
	}
}

func TestComplexityHotspot_JSONSerialization(t *testing.T) {
	hotspot := ComplexityHotspot{
		File:           "test.go",
		Function:       "complex_func",
		Line:           42,
		Complexity:     25,
		ComplexityType: "cyclomatic",
	}

	data, err := json.Marshal(hotspot)
	if err != nil {
		t.Fatalf("Failed to marshal hotspot: %v", err)
	}

	jsonStr := string(data)
	if !strings.Contains(jsonStr, `"file":"test.go"`) {
		t.Error("JSON should contain file")
	}
	if !strings.Contains(jsonStr, `"function":"complex_func"`) {
		t.Error("JSON should contain function")
	}
	if !strings.Contains(jsonStr, `"complexity":25`) {
		t.Error("JSON should contain complexity")
	}
	if !strings.Contains(jsonStr, `"complexity_type":"cyclomatic"`) {
		t.Error("JSON should contain complexity_type")
	}

	var deserialized ComplexityHotspot
	if err := json.Unmarshal(data, &deserialized); err != nil {
		t.Fatalf("Failed to unmarshal hotspot: %v", err)
	}
	if deserialized.Function != "complex_func" {
		t.Errorf("Function = %s, want complex_func", deserialized.Function)
	}
}

func TestFileComplexity_JSONSerialization(t *testing.T) {
	file := FileComplexity{
		Path:            "test.go",
		Language:        "go",
		TotalCyclomatic: 20,
		TotalCognitive:  30,
		Functions: []FunctionComplexity{
			{Name: "func1", StartLine: 10, EndLine: 20, Metrics: ComplexityMetrics{Cyclomatic: 5}},
		},
	}

	data, err := json.Marshal(file)
	if err != nil {
		t.Fatalf("Failed to marshal file: %v", err)
	}

	jsonStr := string(data)
	if !strings.Contains(jsonStr, `"path":"test.go"`) {
		t.Error("JSON should contain path")
	}
	if !strings.Contains(jsonStr, `"language":"go"`) {
		t.Error("JSON should contain language")
	}
	if !strings.Contains(jsonStr, `"functions"`) {
		t.Error("JSON should contain functions")
	}

	var deserialized FileComplexity
	if err := json.Unmarshal(data, &deserialized); err != nil {
		t.Fatalf("Failed to unmarshal file: %v", err)
	}
	if deserialized.Path != "test.go" {
		t.Errorf("Path = %s, want test.go", deserialized.Path)
	}
	if len(deserialized.Functions) != 1 {
		t.Errorf("len(Functions) = %d, want 1", len(deserialized.Functions))
	}
}

func TestExtendedThresholds_JSONSerialization(t *testing.T) {
	thresholds := ExtendedComplexityThresholds{
		CyclomaticWarn:  10,
		CyclomaticError: 20,
		CognitiveWarn:   15,
		CognitiveError:  30,
		NestingMax:      5,
		MethodLength:    50,
	}

	data, err := json.Marshal(thresholds)
	if err != nil {
		t.Fatalf("Failed to marshal thresholds: %v", err)
	}

	jsonStr := string(data)
	if !strings.Contains(jsonStr, `"cyclomatic_warn":10`) {
		t.Error("JSON should contain cyclomatic_warn")
	}
	if !strings.Contains(jsonStr, `"cyclomatic_error":20`) {
		t.Error("JSON should contain cyclomatic_error")
	}

	var deserialized ExtendedComplexityThresholds
	if err := json.Unmarshal(data, &deserialized); err != nil {
		t.Fatalf("Failed to unmarshal thresholds: %v", err)
	}
	if deserialized.CyclomaticWarn != 10 {
		t.Errorf("CyclomaticWarn = %d, want 10", deserialized.CyclomaticWarn)
	}
}
