package defect

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/panbanda/omen/pkg/stats"
)

func TestNew(t *testing.T) {
	analyzer := New(WithChurnDays(90))
	if analyzer == nil {
		t.Fatal("New() returned nil")
	}
	if analyzer.complexity == nil {
		t.Error("analyzer.complexity is nil")
	}
	if analyzer.churn == nil {
		t.Error("analyzer.churn is nil")
	}
	if analyzer.duplicates == nil {
		t.Error("analyzer.duplicates is nil")
	}

	expectedWeights := DefaultWeights()
	if analyzer.weights.Churn != expectedWeights.Churn {
		t.Errorf("weights.Churn = %v, want %v", analyzer.weights.Churn, expectedWeights.Churn)
	}
	if analyzer.weights.Complexity != expectedWeights.Complexity {
		t.Errorf("weights.Complexity = %v, want %v", analyzer.weights.Complexity, expectedWeights.Complexity)
	}
	if analyzer.weights.Duplication != expectedWeights.Duplication {
		t.Errorf("weights.Duplication = %v, want %v", analyzer.weights.Duplication, expectedWeights.Duplication)
	}
	if analyzer.weights.Coupling != expectedWeights.Coupling {
		t.Errorf("weights.Coupling = %v, want %v", analyzer.weights.Coupling, expectedWeights.Coupling)
	}

	analyzer.Close()
}

func TestAnalyzer_Close(t *testing.T) {
	analyzer := New(WithChurnDays(90))
	analyzer.Close()

	// Calling Close multiple times should not panic
	analyzer.Close()
}

func TestAnalyzer_AnalyzeProject_EmptyFiles(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	result, err := analyzer.Analyze(context.Background(), tmpDir, []string{})
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if result == nil {
		t.Fatal("Analyze() returned nil result")
	}

	if result.Summary.TotalFiles != 0 {
		t.Errorf("TotalFiles = %v, want 0", result.Summary.TotalFiles)
	}

	if result.Summary.AvgProbability != 0 {
		t.Errorf("AvgProbability = %v, want 0", result.Summary.AvgProbability)
	}
}

func TestAnalyzer_AnalyzeProject_SingleFile(t *testing.T) {
	tests := []struct {
		name                string
		code                string
		wantRisk            RiskLevel
		wantProbabilityMin  float32
		wantRecommendations int
	}{
		{
			name: "Simple low complexity file",
			code: `package main

func simple() int {
	return 42
}`,
			wantRisk:            RiskLow,
			wantProbabilityMin:  0.0,
			wantRecommendations: 1,
		},
		{
			name: "Complex function",
			code: `package main

func complex(x, y, z int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				for i := 0; i < 10; i++ {
					if i%2 == 0 {
						x += i
					}
				}
				return x + y + z
			}
			return x + y
		}
		return x
	}
	return 0
}`,
			wantRisk:            RiskLow,
			wantProbabilityMin:  0.0,
			wantRecommendations: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := New(WithChurnDays(90))
			defer analyzer.Close()

			result, err := analyzer.Analyze(context.Background(), tmpDir, []string{testFile})
			if err != nil {
				t.Fatalf("Analyze() error = %v", err)
			}

			if len(result.Files) != 1 {
				t.Fatalf("Expected 1 file result, got %d", len(result.Files))
			}

			score := result.Files[0]
			if score.RiskLevel != tt.wantRisk {
				t.Errorf("RiskLevel = %v, want %v", score.RiskLevel, tt.wantRisk)
			}

			if score.Probability < tt.wantProbabilityMin {
				t.Errorf("Probability = %v, want >= %v", score.Probability, tt.wantProbabilityMin)
			}

			if len(score.Recommendations) < tt.wantRecommendations {
				t.Errorf("Recommendations count = %v, want >= %v", len(score.Recommendations), tt.wantRecommendations)
			}

			if score.ContributingFactors == nil {
				t.Error("ContributingFactors is nil")
			}

			if result.Summary.TotalFiles != 1 {
				t.Errorf("TotalFiles = %v, want 1", result.Summary.TotalFiles)
			}
		})
	}
}

func TestAnalyzer_AnalyzeProject_MultipleFiles(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"simple.go": `package main

func simple() int {
	return 42
}`,
		"complex.go": `package main

func veryComplex(x, y, z, a, b, c int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				if a > 0 {
					if b > 0 {
						if c > 0 {
							return x + y + z + a + b + c
						}
					}
				}
			}
		}
	}
	return 0
}`,
		"medium.go": `package main

func medium(x, y int) int {
	if x > 0 {
		if y > 0 {
			return x + y
		}
		return x
	}
	return 0
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	result, err := analyzer.Analyze(context.Background(), tmpDir, filePaths)
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if result.Summary.TotalFiles != 3 {
		t.Errorf("TotalFiles = %v, want 3", result.Summary.TotalFiles)
	}

	if len(result.Files) != 3 {
		t.Errorf("Files count = %v, want 3", len(result.Files))
	}

	if result.Summary.AvgProbability < 0 || result.Summary.AvgProbability > 1 {
		t.Errorf("AvgProbability = %v, want between 0 and 1", result.Summary.AvgProbability)
	}

	if result.Summary.P50Probability < 0 || result.Summary.P50Probability > 1 {
		t.Errorf("P50Probability = %v, want between 0 and 1", result.Summary.P50Probability)
	}

	if result.Summary.P95Probability < 0 || result.Summary.P95Probability > 1 {
		t.Errorf("P95Probability = %v, want between 0 and 1", result.Summary.P95Probability)
	}

	totalRiskCount := result.Summary.HighRiskCount + result.Summary.MediumRiskCount + result.Summary.LowRiskCount
	if totalRiskCount != result.Summary.TotalFiles {
		t.Errorf("Risk counts don't match total files: %d != %d", totalRiskCount, result.Summary.TotalFiles)
	}
}

func TestAnalyzer_AnalyzeProject_NonGitRepo(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")
	code := `package main

func simple() int {
	return 42
}`
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	result, err := analyzer.Analyze(context.Background(), tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("Analyze() should handle non-git repos, got error: %v", err)
	}

	if len(result.Files) != 1 {
		t.Errorf("Expected 1 file result, got %d", len(result.Files))
	}

	// Churn should be 0 for non-git repo
	score := result.Files[0]
	if score.ContributingFactors["churn"] != 0 {
		t.Errorf("Churn factor = %v, want 0 for non-git repo", score.ContributingFactors["churn"])
	}
}

func TestCalculateRiskLevel(t *testing.T) {
	// PMAT-compatible: Low (<0.3), Medium (0.3-0.7), High (>=0.7)
	tests := []struct {
		name        string
		probability float32
		wantRisk    RiskLevel
	}{
		{
			name:        "High risk (was critical)",
			probability: 0.9,
			wantRisk:    RiskHigh,
		},
		{
			name:        "High risk threshold",
			probability: 0.7,
			wantRisk:    RiskHigh,
		},
		{
			name:        "Medium risk",
			probability: 0.5,
			wantRisk:    RiskMedium,
		},
		{
			name:        "Medium risk at boundary",
			probability: 0.3,
			wantRisk:    RiskMedium,
		},
		{
			name:        "Low risk",
			probability: 0.29,
			wantRisk:    RiskLow,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			risk := CalculateRiskLevel(tt.probability)
			if risk != tt.wantRisk {
				t.Errorf("CalculateRiskLevel(%v) = %v, want %v", tt.probability, risk, tt.wantRisk)
			}
		})
	}
}

func TestGenerateRecommendations(t *testing.T) {
	tests := []struct {
		name              string
		metrics           FileMetrics
		probability       float32
		wantContainsChurn bool
		wantContainsComp  bool
		wantContainsDup   bool
		wantContainsCyc   bool
		wantContainsCrit  bool
		wantContainsHigh  bool
	}{
		{
			name: "High churn",
			metrics: FileMetrics{
				ChurnScore: 0.8,
			},
			probability:       0.3,
			wantContainsChurn: true,
		},
		{
			name: "High complexity",
			metrics: FileMetrics{
				Complexity: 25,
			},
			probability:      0.3,
			wantContainsComp: true,
		},
		{
			name: "High duplication",
			metrics: FileMetrics{
				DuplicateRatio: 0.3,
			},
			probability:     0.3,
			wantContainsDup: true,
		},
		{
			name: "High cyclomatic complexity",
			metrics: FileMetrics{
				CyclomaticComplexity: 20,
			},
			probability:     0.3,
			wantContainsCyc: true,
		},
		{
			name: "Critical probability",
			metrics: FileMetrics{
				Complexity: 10,
			},
			probability:      0.85,
			wantContainsCrit: true,
		},
		{
			name: "High probability",
			metrics: FileMetrics{
				Complexity: 10,
			},
			probability:      0.65,
			wantContainsHigh: true,
		},
		{
			name: "All clean",
			metrics: FileMetrics{
				ChurnScore:           0.1,
				Complexity:           5,
				DuplicateRatio:       0.05,
				CyclomaticComplexity: 3,
			},
			probability: 0.2,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			recs := generateRecommendations(tt.metrics, tt.probability)

			if len(recs) == 0 {
				t.Error("generateRecommendations() returned empty recommendations")
			}

			hasChurn := false
			hasComp := false
			hasDup := false
			hasCyc := false
			hasCrit := false
			hasHigh := false

			for _, rec := range recs {
				if containsIgnoreCase(rec, "churn") {
					hasChurn = true
				}
				if containsIgnoreCase(rec, "complexity") || containsIgnoreCase(rec, "refactoring") {
					hasComp = true
				}
				if containsIgnoreCase(rec, "duplication") {
					hasDup = true
				}
				if containsIgnoreCase(rec, "control flow") || containsIgnoreCase(rec, "conditional") {
					hasCyc = true
				}
				if containsIgnoreCase(rec, "CRITICAL") {
					hasCrit = true
				}
				if containsIgnoreCase(rec, "HIGH RISK") {
					hasHigh = true
				}
			}

			if tt.wantContainsChurn && !hasChurn {
				t.Errorf("Expected churn recommendation, got: %v", recs)
			}
			if tt.wantContainsComp && !hasComp {
				t.Errorf("Expected complexity recommendation, got: %v", recs)
			}
			if tt.wantContainsDup && !hasDup {
				t.Errorf("Expected duplication recommendation, got: %v", recs)
			}
			if tt.wantContainsCyc && !hasCyc {
				t.Errorf("Expected cyclomatic recommendation, got: %v", recs)
			}
			if tt.wantContainsCrit && !hasCrit {
				t.Errorf("Expected critical recommendation, got: %v", recs)
			}
			if tt.wantContainsHigh && !hasHigh {
				t.Errorf("Expected high risk recommendation, got: %v", recs)
			}
		})
	}
}

func TestPercentile(t *testing.T) {
	tests := []struct {
		name   string
		sorted []float64
		p      int
		want   float64
	}{
		{
			name:   "Empty slice",
			sorted: []float64{},
			p:      50,
			want:   0,
		},
		{
			name:   "Single value",
			sorted: []float64{0.5},
			p:      50,
			want:   0.5,
		},
		{
			name:   "P50 of sorted values",
			sorted: []float64{0.1, 0.2, 0.3, 0.4, 0.5},
			p:      50,
			want:   0.3,
		},
		{
			name:   "P95 of sorted values",
			sorted: []float64{0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0},
			p:      95,
			want:   1.0,
		},
		{
			name:   "P0 should return first",
			sorted: []float64{0.1, 0.2, 0.3, 0.4, 0.5},
			p:      0,
			want:   0.1,
		},
		{
			name:   "P100 should return last",
			sorted: []float64{0.1, 0.2, 0.3, 0.4, 0.5},
			p:      100,
			want:   0.5,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := stats.Percentile(tt.sorted, tt.p)
			if got != tt.want {
				t.Errorf("stats.Percentile() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestAnalyzer_ContributingFactors(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

func complex(x, y, z int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				return x + y + z
			}
			return x + y
		}
		return x
	}
	return 0
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	result, err := analyzer.Analyze(context.Background(), tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	if len(result.Files) != 1 {
		t.Fatalf("Expected 1 file result, got %d", len(result.Files))
	}

	score := result.Files[0]

	requiredFactors := []string{"churn", "complexity", "duplication", "coupling"}
	for _, factor := range requiredFactors {
		if _, ok := score.ContributingFactors[factor]; !ok {
			t.Errorf("Missing contributing factor: %s", factor)
		}
	}

	for factor, value := range score.ContributingFactors {
		if value < 0 {
			t.Errorf("Contributing factor %s has negative value: %v", factor, value)
		}
	}
}

func TestAnalyzer_WeightsIntegration(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

func simple() int {
	return 42
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	result, err := analyzer.Analyze(context.Background(), tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	expectedWeights := DefaultWeights()
	if result.Weights.Churn != expectedWeights.Churn {
		t.Errorf("Weights.Churn = %v, want %v", result.Weights.Churn, expectedWeights.Churn)
	}
	if result.Weights.Complexity != expectedWeights.Complexity {
		t.Errorf("Weights.Complexity = %v, want %v", result.Weights.Complexity, expectedWeights.Complexity)
	}
	if result.Weights.Duplication != expectedWeights.Duplication {
		t.Errorf("Weights.Duplication = %v, want %v", result.Weights.Duplication, expectedWeights.Duplication)
	}
	if result.Weights.Coupling != expectedWeights.Coupling {
		t.Errorf("Weights.Coupling = %v, want %v", result.Weights.Coupling, expectedWeights.Coupling)
	}
	if result.Weights.Ownership != expectedWeights.Ownership {
		t.Errorf("Weights.Ownership = %v, want %v", result.Weights.Ownership, expectedWeights.Ownership)
	}

	// Weights should sum to 1.0
	sum := result.Weights.Churn + result.Weights.Complexity + result.Weights.Duplication + result.Weights.Coupling + result.Weights.Ownership
	if sum != 1.0 {
		t.Errorf("Weights sum = %v, want 1.0", sum)
	}
}

func TestAnalyzer_ProbabilityBounds(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"test1.go": `package main
func f1() int { return 1 }`,
		"test2.go": `package main
func f2() int {
	if true {
		if true {
			if true {
				return 1
			}
		}
	}
	return 0
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	result, err := analyzer.Analyze(context.Background(), tmpDir, filePaths)
	if err != nil {
		t.Fatalf("Analyze() error = %v", err)
	}

	for _, score := range result.Files {
		if score.Probability < 0 || score.Probability > 1 {
			t.Errorf("Probability for %s = %v, want between 0 and 1", score.FilePath, score.Probability)
		}
	}

	if result.Summary.AvgProbability < 0 || result.Summary.AvgProbability > 1 {
		t.Errorf("AvgProbability = %v, want between 0 and 1", result.Summary.AvgProbability)
	}

	if result.Summary.P50Probability < 0 || result.Summary.P50Probability > 1 {
		t.Errorf("P50Probability = %v, want between 0 and 1", result.Summary.P50Probability)
	}

	if result.Summary.P95Probability < 0 || result.Summary.P95Probability > 1 {
		t.Errorf("P95Probability = %v, want between 0 and 1", result.Summary.P95Probability)
	}
}

func TestCalculateProbability(t *testing.T) {
	weights := DefaultWeights()

	// Test that duplicate ratio is capped at 1.0
	metrics := FileMetrics{
		FilePath:       "test.go",
		DuplicateRatio: 1.5, // This should be capped
	}

	prob := CalculateProbability(metrics, weights)

	if prob > 1.0 {
		t.Errorf("Probability = %v, want <= 1.0", prob)
	}
}

func TestCalculateConfidence(t *testing.T) {
	tests := []struct {
		name    string
		metrics FileMetrics
		wantMin float32
		wantMax float32
	}{
		{
			name:    "Full metrics",
			metrics: FileMetrics{LinesOfCode: 100, ChurnScore: 0.5, AfferentCoupling: 2},
			wantMin: 0.9,
			wantMax: 1.0,
		},
		{
			name:    "Small file",
			metrics: FileMetrics{LinesOfCode: 5},
			wantMin: 0.3,
			wantMax: 0.5,
		},
		{
			name:    "Medium file",
			metrics: FileMetrics{LinesOfCode: 30},
			wantMin: 0.5,
			wantMax: 0.8,
		},
		{
			name:    "No churn",
			metrics: FileMetrics{LinesOfCode: 100, ChurnScore: 0},
			wantMin: 0.7,
			wantMax: 0.9,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			conf := CalculateConfidence(tt.metrics)
			if conf < tt.wantMin || conf > tt.wantMax {
				t.Errorf("CalculateConfidence() = %v, want between %v and %v", conf, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestNormalizeFunctions(t *testing.T) {
	// Test NormalizeChurn
	if v := NormalizeChurn(0.0); v != 0.0 {
		t.Errorf("NormalizeChurn(0.0) = %v, want 0.0", v)
	}
	if v := NormalizeChurn(1.0); v != 1.0 {
		t.Errorf("NormalizeChurn(1.0) = %v, want 1.0", v)
	}
	if v := NormalizeChurn(0.5); v < 0.5 || v > 0.8 {
		t.Errorf("NormalizeChurn(0.5) = %v, want ~0.7", v)
	}

	// Test NormalizeDuplication (direct clamp)
	if v := NormalizeDuplication(-0.5); v != 0 {
		t.Errorf("NormalizeDuplication(-0.5) = %v, want 0", v)
	}
	if v := NormalizeDuplication(1.5); v != 1 {
		t.Errorf("NormalizeDuplication(1.5) = %v, want 1", v)
	}
	if v := NormalizeDuplication(0.5); v != 0.5 {
		t.Errorf("NormalizeDuplication(0.5) = %v, want 0.5", v)
	}
}

func TestAnalysis_ToReport(t *testing.T) {
	analysis := &Analysis{
		Files: []Score{
			{
				FilePath:    "test.go",
				Probability: 0.5,
				RiskLevel:   RiskMedium,
				ContributingFactors: map[string]float32{
					"churn":      0.1,
					"complexity": 0.2,
				},
			},
		},
		Summary: Summary{
			TotalFiles:      1,
			HighRiskCount:   0,
			MediumRiskCount: 1,
			LowRiskCount:    0,
		},
	}

	report := analysis.ToReport()

	if report.TotalFiles != 1 {
		t.Errorf("ToReport().TotalFiles = %d, want 1", report.TotalFiles)
	}
	if report.MediumRiskFiles != 1 {
		t.Errorf("ToReport().MediumRiskFiles = %d, want 1", report.MediumRiskFiles)
	}
	if len(report.FilePredictions) != 1 {
		t.Errorf("ToReport().FilePredictions length = %d, want 1", len(report.FilePredictions))
	}
}

// Helper function for case-insensitive substring search
func containsIgnoreCase(s, substr string) bool {
	return strings.Contains(strings.ToLower(s), strings.ToLower(substr))
}

func BenchmarkAnalyzer_AnalyzeProject(b *testing.B) {
	tmpDir := b.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

func complex(x, y, z int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				for i := 0; i < 10; i++ {
					if i%2 == 0 {
						x += i
					}
				}
				return x + y + z
			}
			return x + y
		}
		return x
	}
	return 0
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		b.Fatalf("failed to write test file: %v", err)
	}

	analyzer := New(WithChurnDays(90))
	defer analyzer.Close()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := analyzer.Analyze(context.Background(), tmpDir, []string{testFile})
		if err != nil {
			b.Fatalf("Analyze() error = %v", err)
		}
	}
}

func BenchmarkGenerateRecommendations(b *testing.B) {
	metrics := FileMetrics{
		ChurnScore:           0.8,
		Complexity:           25,
		DuplicateRatio:       0.3,
		CyclomaticComplexity: 20,
	}
	probability := float32(0.7)

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = generateRecommendations(metrics, probability)
	}
}
