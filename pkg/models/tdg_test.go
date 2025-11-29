package models

import (
	"testing"
)

func TestGradeFromScore(t *testing.T) {
	tests := []struct {
		score    float32
		expected Grade
	}{
		{95.0, GradeAPlus},
		{97.5, GradeAPlus},
		{90.0, GradeA},
		{94.9, GradeA},
		{85.0, GradeAMinus},
		{89.9, GradeAMinus},
		{80.0, GradeBPlus},
		{84.9, GradeBPlus},
		{75.0, GradeB},
		{79.9, GradeB},
		{70.0, GradeBMinus},
		{74.9, GradeBMinus},
		{65.0, GradeCPlus},
		{69.9, GradeCPlus},
		{60.0, GradeC},
		{64.9, GradeC},
		{55.0, GradeCMinus},
		{59.9, GradeCMinus},
		{50.0, GradeD},
		{54.9, GradeD},
		{45.0, GradeF},
		{0.0, GradeF},
	}

	for _, tc := range tests {
		t.Run("", func(t *testing.T) {
			got := GradeFromScore(tc.score)
			if got != tc.expected {
				t.Errorf("GradeFromScore(%v) = %v, want %v", tc.score, got, tc.expected)
			}
		})
	}
}

func TestLanguageFromExtension(t *testing.T) {
	tests := []struct {
		path     string
		expected Language
	}{
		{"main.rs", LanguageRust},
		{"main.go", LanguageGo},
		{"main.py", LanguagePython},
		{"main.js", LanguageJavaScript},
		{"main.ts", LanguageTypeScript},
		{"main.tsx", LanguageTypeScript},
		{"main.java", LanguageJava},
		{"main.c", LanguageC},
		{"main.cpp", LanguageCpp},
		{"main.cc", LanguageCpp},
		{"main.cs", LanguageCSharp},
		{"main.rb", LanguageRuby},
		{"main.php", LanguagePHP},
		{"main.swift", LanguageSwift},
		{"main.kt", LanguageKotlin},
		{"main.kts", LanguageKotlin},
		{"main.txt", LanguageUnknown},
		{"main", LanguageUnknown},
	}

	for _, tc := range tests {
		t.Run(tc.path, func(t *testing.T) {
			got := LanguageFromExtension(tc.path)
			if got != tc.expected {
				t.Errorf("LanguageFromExtension(%q) = %v, want %v", tc.path, got, tc.expected)
			}
		})
	}
}

func TestLanguageConfidence(t *testing.T) {
	if LanguageUnknown.Confidence() != 0.5 {
		t.Errorf("Unknown language confidence = %v, want 0.5", LanguageUnknown.Confidence())
	}

	if LanguageGo.Confidence() != 0.95 {
		t.Errorf("Go language confidence = %v, want 0.95", LanguageGo.Confidence())
	}
}

func TestPenaltyTracker(t *testing.T) {
	tracker := NewPenaltyTracker()

	// First application should succeed
	amount := tracker.Apply("issue1", MetricStructuralComplexity, 3.5, "High complexity")
	if amount != 3.5 {
		t.Errorf("First Apply() = %v, want 3.5", amount)
	}

	// Duplicate should return 0
	amount = tracker.Apply("issue1", MetricStructuralComplexity, 3.5, "High complexity")
	if amount != 0 {
		t.Errorf("Duplicate Apply() = %v, want 0", amount)
	}

	// Different issue should succeed
	amount = tracker.Apply("issue2", MetricDuplication, 2.0, "Code duplication")
	if amount != 2.0 {
		t.Errorf("Second Apply() = %v, want 2.0", amount)
	}

	attrs := tracker.GetAttributions()
	if len(attrs) != 2 {
		t.Errorf("GetAttributions() len = %v, want 2", len(attrs))
	}
}

func TestTdgScoreDefault(t *testing.T) {
	score := NewTdgScore()

	if score.Total != 100.0 {
		t.Errorf("Default Total = %v, want 100.0", score.Total)
	}

	if score.Grade != GradeAPlus {
		t.Errorf("Default Grade = %v, want A+", score.Grade)
	}

	if score.Confidence != 1.0 {
		t.Errorf("Default Confidence = %v, want 1.0", score.Confidence)
	}
}

func TestTdgScoreCalculateTotal(t *testing.T) {
	score := TdgScore{
		StructuralComplexity:  18.0,
		SemanticComplexity:    13.0,
		DuplicationRatio:      14.0,
		CouplingScore:         14.0,
		DocCoverage:           4.0,
		ConsistencyScore:      9.0,
		HotspotScore:          9.0,
		TemporalCouplingScore: 9.0,
		EntropyScore:          0.0,
	}

	score.CalculateTotal()

	// Expected: 18+13+14+14+4+9+9+9+0 = 90.0
	expectedTotal := float32(90.0)
	if score.Total != expectedTotal {
		t.Errorf("CalculateTotal() Total = %v, want %v", score.Total, expectedTotal)
	}

	if score.Grade != GradeA {
		t.Errorf("CalculateTotal() Grade = %v, want A", score.Grade)
	}
}

func TestTdgScoreCriticalDefects(t *testing.T) {
	score := TdgScore{
		StructuralComplexity:  20.0,
		SemanticComplexity:    15.0,
		DuplicationRatio:      15.0,
		CouplingScore:         15.0,
		DocCoverage:           5.0,
		ConsistencyScore:      10.0,
		HotspotScore:          10.0,
		TemporalCouplingScore: 10.0,
		HasCriticalDefects:    true,
		CriticalDefectsCount:  3,
	}

	score.CalculateTotal()

	if score.Total != 0.0 {
		t.Errorf("Critical defects Total = %v, want 0.0", score.Total)
	}

	if score.Grade != GradeF {
		t.Errorf("Critical defects Grade = %v, want F", score.Grade)
	}
}

func TestTdgScoreSetMetric(t *testing.T) {
	score := NewTdgScore()

	score.SetMetric(MetricStructuralComplexity, 15.0)
	if score.StructuralComplexity != 15.0 {
		t.Errorf("SetMetric StructuralComplexity = %v, want 15.0", score.StructuralComplexity)
	}

	score.SetMetric(MetricSemanticComplexity, 12.0)
	if score.SemanticComplexity != 12.0 {
		t.Errorf("SetMetric SemanticComplexity = %v, want 12.0", score.SemanticComplexity)
	}

	score.SetMetric(MetricDuplication, 10.0)
	if score.DuplicationRatio != 10.0 {
		t.Errorf("SetMetric DuplicationRatio = %v, want 10.0", score.DuplicationRatio)
	}

	score.SetMetric(MetricCoupling, 8.0)
	if score.CouplingScore != 8.0 {
		t.Errorf("SetMetric CouplingScore = %v, want 8.0", score.CouplingScore)
	}

	score.SetMetric(MetricDocumentation, 5.0)
	if score.DocCoverage != 5.0 {
		t.Errorf("SetMetric DocCoverage = %v, want 5.0", score.DocCoverage)
	}

	score.SetMetric(MetricConsistency, 6.0)
	if score.ConsistencyScore != 6.0 {
		t.Errorf("SetMetric ConsistencyScore = %v, want 6.0", score.ConsistencyScore)
	}
}

func TestTdgScoreClamp(t *testing.T) {
	score := TdgScore{
		StructuralComplexity:  50.0, // Will be clamped to 20
		SemanticComplexity:    -5.0, // Will be clamped to 0
		DuplicationRatio:      15.0,
		CouplingScore:         15.0,
		DocCoverage:           5.0,
		ConsistencyScore:      10.0,
		HotspotScore:          10.0,
		TemporalCouplingScore: 10.0,
	}

	score.CalculateTotal()

	if score.StructuralComplexity != 20.0 {
		t.Errorf("StructuralComplexity clamped = %v, want 20.0", score.StructuralComplexity)
	}

	if score.SemanticComplexity != 0.0 {
		t.Errorf("SemanticComplexity clamped = %v, want 0.0", score.SemanticComplexity)
	}
}

func TestProjectScoreAggregate(t *testing.T) {
	scores := []TdgScore{
		{Total: 80.0, Language: LanguageGo},
		{Total: 90.0, Language: LanguageGo},
		{Total: 70.0, Language: LanguageRust},
	}

	for i := range scores {
		scores[i].Grade = GradeFromScore(scores[i].Total)
	}

	project := AggregateProjectScore(scores)

	if project.TotalFiles != 3 {
		t.Errorf("TotalFiles = %v, want 3", project.TotalFiles)
	}

	expectedAvg := float32(80.0)
	if project.AverageScore != expectedAvg {
		t.Errorf("AverageScore = %v, want %v", project.AverageScore, expectedAvg)
	}

	if project.LanguageDistribution[LanguageGo] != 2 {
		t.Errorf("LanguageDistribution[Go] = %v, want 2", project.LanguageDistribution[LanguageGo])
	}

	if project.LanguageDistribution[LanguageRust] != 1 {
		t.Errorf("LanguageDistribution[Rust] = %v, want 1", project.LanguageDistribution[LanguageRust])
	}

	if project.GradeDistribution[GradeBPlus] != 1 {
		t.Errorf("GradeDistribution[B+] = %v, want 1", project.GradeDistribution[GradeBPlus])
	}

	if project.GradeDistribution[GradeA] != 1 {
		t.Errorf("GradeDistribution[A] = %v, want 1", project.GradeDistribution[GradeA])
	}

	if project.GradeDistribution[GradeBMinus] != 1 {
		t.Errorf("GradeDistribution[B-] = %v, want 1", project.GradeDistribution[GradeBMinus])
	}
}

func TestProjectScoreAverage(t *testing.T) {
	scores := []TdgScore{
		{
			StructuralComplexity:  18.0,
			SemanticComplexity:    13.0,
			DuplicationRatio:      14.0,
			CouplingScore:         12.0,
			DocCoverage:           4.0,
			ConsistencyScore:      8.0,
			HotspotScore:          9.0,
			TemporalCouplingScore: 9.0,
			Confidence:            0.9,
		},
		{
			StructuralComplexity:  20.0,
			SemanticComplexity:    14.0,
			DuplicationRatio:      14.0,
			CouplingScore:         14.0,
			DocCoverage:           4.0,
			ConsistencyScore:      10.0,
			HotspotScore:          10.0,
			TemporalCouplingScore: 10.0,
			Confidence:            0.95,
		},
	}

	project := AggregateProjectScore(scores)
	avg := project.Average()

	// Expected averages
	expectedStructural := float32(19.0) // (18+20)/2
	if avg.StructuralComplexity != expectedStructural {
		t.Errorf("Average StructuralComplexity = %v, want %v", avg.StructuralComplexity, expectedStructural)
	}

	expectedConfidence := float32(0.925)
	// Use approximate comparison for floating point
	if abs32(avg.Confidence-expectedConfidence) > 0.001 {
		t.Errorf("Average Confidence = %v, want ~%v", avg.Confidence, expectedConfidence)
	}
}

func TestTdgComparison(t *testing.T) {
	source1 := TdgScore{
		Total:                80.0,
		StructuralComplexity: 20.0,
		SemanticComplexity:   15.0,
		DuplicationRatio:     18.0,
		DocCoverage:          8.0,
		FilePath:             "old.go",
	}

	source2 := TdgScore{
		Total:                90.0,
		StructuralComplexity: 24.0, // Improved
		SemanticComplexity:   18.0, // Improved
		DuplicationRatio:     16.0, // Regression
		DocCoverage:          10.0, // Improved
		FilePath:             "new.go",
	}

	comparison := NewTdgComparison(source1, source2)

	if comparison.Delta != 10.0 {
		t.Errorf("Delta = %v, want 10.0", comparison.Delta)
	}

	if comparison.ImprovementPercentage != 12.5 {
		t.Errorf("ImprovementPercentage = %v, want 12.5", comparison.ImprovementPercentage)
	}

	if comparison.Winner != "new.go" {
		t.Errorf("Winner = %v, want 'new.go'", comparison.Winner)
	}

	if len(comparison.Improvements) < 3 {
		t.Errorf("Expected at least 3 improvements, got %d", len(comparison.Improvements))
	}

	if len(comparison.Regressions) < 1 {
		t.Errorf("Expected at least 1 regression, got %d", len(comparison.Regressions))
	}
}

func TestClampFloat32(t *testing.T) {
	tests := []struct {
		value, min, max, expected float32
	}{
		{5.0, 0.0, 10.0, 5.0},   // Within range
		{-5.0, 0.0, 10.0, 0.0},  // Below min
		{15.0, 0.0, 10.0, 10.0}, // Above max
		{0.0, 0.0, 10.0, 0.0},   // At min
		{10.0, 0.0, 10.0, 10.0}, // At max
	}

	for _, tc := range tests {
		got := clampFloat32(tc.value, tc.min, tc.max)
		if got != tc.expected {
			t.Errorf("clampFloat32(%v, %v, %v) = %v, want %v",
				tc.value, tc.min, tc.max, got, tc.expected)
		}
	}
}

func abs32(x float32) float32 {
	if x < 0 {
		return -x
	}
	return x
}
