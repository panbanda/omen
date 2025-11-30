package analyzer

import (
	"testing"

	"github.com/panbanda/omen/pkg/models"
)

func TestIsBugFixCommit(t *testing.T) {
	tests := []struct {
		message string
		isFix   bool
	}{
		{"fix: resolve null pointer exception", true},
		{"Fix bug in login handler", true},
		{"fixes #123", true},
		{"bugfix: handle edge case", true},
		{"patch security vulnerability", true},
		{"resolve race condition", true},
		{"closes #456", true},
		{"Fixed memory leak", true},
		{"Fixing typo", true},
		{"Error handling improvement", true},
		{"crash fix for iOS", true},
		{"defect: validation missing", true},
		{"issue: timeout not respected", true},
		{"feat: add new feature", false},
		{"refactor: clean up code", false},
		{"docs: update README", false},
		{"chore: update dependencies", false},
		{"test: add unit tests", false},
		{"style: format code", false},
	}

	for _, tt := range tests {
		t.Run(tt.message, func(t *testing.T) {
			result := isBugFixCommit(tt.message)
			if result != tt.isFix {
				t.Errorf("isBugFixCommit(%q) = %v, expected %v", tt.message, result, tt.isFix)
			}
		})
	}
}

func TestCalculateEntropy(t *testing.T) {
	tests := []struct {
		name          string
		linesPerFile  map[string]int
		expectedRange [2]float64 // min, max
	}{
		{
			name:          "empty",
			linesPerFile:  map[string]int{},
			expectedRange: [2]float64{0, 0},
		},
		{
			name: "single file",
			linesPerFile: map[string]int{
				"file.go": 100,
			},
			expectedRange: [2]float64{0, 0}, // entropy = 0 for single file
		},
		{
			name: "equal distribution - 2 files",
			linesPerFile: map[string]int{
				"a.go": 50,
				"b.go": 50,
			},
			expectedRange: [2]float64{0.9, 1.1}, // should be ~1.0 (log2(2))
		},
		{
			name: "unequal distribution",
			linesPerFile: map[string]int{
				"a.go": 90,
				"b.go": 10,
			},
			expectedRange: [2]float64{0.4, 0.5}, // lower entropy
		},
		{
			name: "equal distribution - 4 files",
			linesPerFile: map[string]int{
				"a.go": 25,
				"b.go": 25,
				"c.go": 25,
				"d.go": 25,
			},
			expectedRange: [2]float64{1.9, 2.1}, // should be ~2.0 (log2(4))
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			entropy := models.CalculateEntropy(tt.linesPerFile)
			if entropy < tt.expectedRange[0] || entropy > tt.expectedRange[1] {
				t.Errorf("CalculateEntropy() = %f, expected in range [%f, %f]",
					entropy, tt.expectedRange[0], tt.expectedRange[1])
			}
		})
	}
}

func TestCalculateJITRisk(t *testing.T) {
	weights := models.DefaultJITWeights()
	norm := models.NormalizationStats{
		MaxLinesAdded:       1000,
		MaxLinesDeleted:     500,
		MaxNumFiles:         50,
		MaxUniqueChanges:    100,
		MaxNumDevelopers:    10,
		MaxAuthorExperience: 50,
		MaxEntropy:          4.0,
	}

	tests := []struct {
		name          string
		features      models.CommitFeatures
		expectedRange [2]float64
	}{
		{
			name: "low risk - small change, experienced author",
			features: models.CommitFeatures{
				IsFix:            false,
				LinesAdded:       10,
				LinesDeleted:     5,
				NumFiles:         1,
				UniqueChanges:    5,
				NumDevelopers:    1,
				AuthorExperience: 50, // max experience
				Entropy:          0,
			},
			expectedRange: [2]float64{0, 0.2},
		},
		{
			name: "high risk - bug fix, large change, new author",
			features: models.CommitFeatures{
				IsFix:            true,
				LinesAdded:       1000,
				LinesDeleted:     500,
				NumFiles:         50,
				UniqueChanges:    100,
				NumDevelopers:    10,
				AuthorExperience: 0, // no experience
				Entropy:          4.0,
			},
			expectedRange: [2]float64{0.8, 1.0},
		},
		{
			name: "medium risk - moderate change",
			features: models.CommitFeatures{
				IsFix:            false,
				LinesAdded:       200,
				LinesDeleted:     100,
				NumFiles:         10,
				UniqueChanges:    20,
				NumDevelopers:    3,
				AuthorExperience: 25,
				Entropy:          2.0,
			},
			// Score breakdown:
			// FIX=0, Entropy=0.10, LA=0.04, LD=0.01, NF=0.02, NUC=0.02, NDEV=0.015, EXP=0.025
			// Total ~0.23
			expectedRange: [2]float64{0.2, 0.3},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			score := models.CalculateJITRisk(tt.features, weights, norm)
			if score < tt.expectedRange[0] || score > tt.expectedRange[1] {
				t.Errorf("CalculateJITRisk() = %f, expected in range [%f, %f]",
					score, tt.expectedRange[0], tt.expectedRange[1])
			}
		})
	}
}

func TestGetJITRiskLevel(t *testing.T) {
	tests := []struct {
		score    float64
		expected models.JITRiskLevel
	}{
		{0.0, models.JITRiskLow},
		{0.39, models.JITRiskLow},
		{0.4, models.JITRiskMedium},
		{0.69, models.JITRiskMedium},
		{0.7, models.JITRiskHigh},
		{1.0, models.JITRiskHigh},
	}

	for _, tt := range tests {
		result := models.GetJITRiskLevel(tt.score)
		if result != tt.expected {
			t.Errorf("GetJITRiskLevel(%f) = %s, expected %s", tt.score, result, tt.expected)
		}
	}
}

func TestDefaultJITWeights(t *testing.T) {
	weights := models.DefaultJITWeights()

	// Verify weights sum to 1.0
	sum := weights.FIX + weights.Entropy + weights.LA + weights.NUC +
		weights.NF + weights.LD + weights.NDEV + weights.EXP

	if sum < 0.99 || sum > 1.01 {
		t.Errorf("JIT weights sum to %f, expected 1.0", sum)
	}

	// Verify specific weights from requirements
	if weights.FIX != 0.25 {
		t.Errorf("FIX weight = %f, expected 0.25", weights.FIX)
	}
	if weights.Entropy != 0.20 {
		t.Errorf("Entropy weight = %f, expected 0.20", weights.Entropy)
	}
	if weights.LA != 0.20 {
		t.Errorf("LA weight = %f, expected 0.20", weights.LA)
	}
}

func TestTruncateMessage(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{"Short message", "Short message"},
		{"Line one\nLine two", "Line one"},
		{
			"This is a very long commit message that exceeds the eighty character limit for display purposes",
			"This is a very long commit message that exceeds the eighty character limit fo...",
		},
		{"  Whitespace  \n", "Whitespace"},
	}

	for _, tt := range tests {
		result := truncateMessage(tt.input)
		if result != tt.expected {
			t.Errorf("truncateMessage(%q) = %q, expected %q", tt.input, result, tt.expected)
		}
	}
}

func TestSafeNormalize(t *testing.T) {
	tests := []struct {
		value    float64
		max      float64
		expected float64
	}{
		{0, 100, 0},
		{50, 100, 0.5},
		{100, 100, 1.0},
		{150, 100, 1.0}, // clamped
		{50, 0, 0},      // zero max
		{50, -10, 0},    // negative max
	}

	for _, tt := range tests {
		result := safeNormalize(tt.value, tt.max)
		if result != tt.expected {
			t.Errorf("safeNormalize(%f, %f) = %f, expected %f",
				tt.value, tt.max, result, tt.expected)
		}
	}
}

func TestGenerateJITRecommendations(t *testing.T) {
	features := models.CommitFeatures{
		IsFix: true,
	}
	factors := map[string]float64{
		"entropy":    0.20,
		"experience": 0.05,
	}
	score := 0.75

	recs := models.GenerateJITRecommendations(features, score, factors)

	if len(recs) == 0 {
		t.Error("Expected recommendations for high-risk commit")
	}

	// Should include bug fix recommendation
	found := false
	for _, rec := range recs {
		if rec == "Bug fix commit - ensure comprehensive testing of the fix" {
			found = true
			break
		}
	}
	if !found {
		t.Error("Expected bug fix recommendation")
	}
}
