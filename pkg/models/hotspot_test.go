package models

import (
	"testing"
	"time"
)

func TestHotspotAnalysis_CalculateSummary(t *testing.T) {
	tests := []struct {
		name         string
		files        []FileHotspot
		wantTotal    int
		wantHotspots int
		wantMax      float64
		wantAvg      float64
	}{
		{
			name:         "empty",
			files:        []FileHotspot{},
			wantTotal:    0,
			wantHotspots: 0,
			wantMax:      0,
			wantAvg:      0,
		},
		{
			name: "single file",
			files: []FileHotspot{
				{Path: "a.go", HotspotScore: 0.7},
			},
			wantTotal:    1,
			wantHotspots: 1,
			wantMax:      0.7,
			wantAvg:      0.7,
		},
		{
			name: "multiple files sorted descending",
			files: []FileHotspot{
				{Path: "a.go", HotspotScore: 0.9},
				{Path: "b.go", HotspotScore: 0.6},
				{Path: "c.go", HotspotScore: 0.4},
				{Path: "d.go", HotspotScore: 0.1},
			},
			wantTotal:    4,
			wantHotspots: 3, // 0.9, 0.6, and 0.4 are >= 0.4 (HighHotspotThreshold)
			wantMax:      0.9,
			wantAvg:      0.5, // (0.9 + 0.6 + 0.4 + 0.1) / 4 = 0.5
		},
		{
			name: "all below threshold",
			files: []FileHotspot{
				{Path: "a.go", HotspotScore: 0.35},
				{Path: "b.go", HotspotScore: 0.25},
				{Path: "c.go", HotspotScore: 0.15},
			},
			wantTotal:    3,
			wantHotspots: 0,
			wantMax:      0.35,
			wantAvg:      0.25,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analysis := &HotspotAnalysis{
				GeneratedAt: time.Now(),
				PeriodDays:  30,
				Files:       tt.files,
			}
			analysis.CalculateSummary()

			if analysis.Summary.TotalFiles != tt.wantTotal {
				t.Errorf("TotalFiles = %d, want %d", analysis.Summary.TotalFiles, tt.wantTotal)
			}
			if analysis.Summary.HotspotCount != tt.wantHotspots {
				t.Errorf("HotspotCount = %d, want %d", analysis.Summary.HotspotCount, tt.wantHotspots)
			}
			if analysis.Summary.MaxHotspotScore != tt.wantMax {
				t.Errorf("MaxHotspotScore = %f, want %f", analysis.Summary.MaxHotspotScore, tt.wantMax)
			}
			if tt.wantTotal > 0 && analysis.Summary.AvgHotspotScore != tt.wantAvg {
				t.Errorf("AvgHotspotScore = %f, want %f", analysis.Summary.AvgHotspotScore, tt.wantAvg)
			}
		})
	}
}

func TestHotspotAnalysis_CalculateSummary_Percentiles(t *testing.T) {
	// Test that percentiles are calculated correctly
	analysis := &HotspotAnalysis{
		Files: []FileHotspot{
			{Path: "a.go", HotspotScore: 0.9},
			{Path: "b.go", HotspotScore: 0.7},
			{Path: "c.go", HotspotScore: 0.5},
			{Path: "d.go", HotspotScore: 0.3},
			{Path: "e.go", HotspotScore: 0.1},
		},
	}
	analysis.CalculateSummary()

	// P50 should be the median (0.5)
	if analysis.Summary.P50HotspotScore != 0.5 {
		t.Errorf("P50HotspotScore = %f, want 0.5", analysis.Summary.P50HotspotScore)
	}

	// P90 should be high (0.9)
	if analysis.Summary.P90HotspotScore != 0.9 {
		t.Errorf("P90HotspotScore = %f, want 0.9", analysis.Summary.P90HotspotScore)
	}
}

func TestDefaultHotspotScoreThreshold(t *testing.T) {
	// Default threshold should be the High threshold (0.4)
	if DefaultHotspotScoreThreshold != HighHotspotThreshold {
		t.Errorf("DefaultHotspotScoreThreshold = %f, want %f", DefaultHotspotScoreThreshold, HighHotspotThreshold)
	}
}

func TestNormalizeChurnCDF(t *testing.T) {
	tests := []struct {
		commits int
		wantMin float64
		wantMax float64
	}{
		{0, 0.0, 0.0},     // No commits
		{1, 0.29, 0.31},   // Single commit ~0.30
		{2, 0.49, 0.51},   // Two commits ~0.50
		{5, 0.74, 0.76},   // Five commits ~0.75
		{10, 0.91, 0.93},  // Ten commits ~0.92
		{50, 0.99, 1.01},  // Fifty commits ~1.0
		{100, 0.99, 1.01}, // Above max still 1.0
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := NormalizeChurnCDF(tt.commits)
			if got < tt.wantMin || got > tt.wantMax {
				t.Errorf("NormalizeChurnCDF(%d) = %f, want between %f and %f",
					tt.commits, got, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestNormalizeComplexityCDF(t *testing.T) {
	tests := []struct {
		avgCognitive float64
		wantMin      float64
		wantMax      float64
	}{
		{0, 0.0, 0.0},     // No complexity
		{1, 0.09, 0.11},   // Trivial ~0.10
		{5, 0.49, 0.51},   // Moderate ~0.50
		{10, 0.79, 0.81},  // Complex ~0.80
		{15, 0.89, 0.91},  // High ~0.90
		{50, 0.99, 1.01},  // Extreme ~1.0
		{100, 0.99, 1.01}, // Above max still 1.0
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := NormalizeComplexityCDF(tt.avgCognitive)
			if got < tt.wantMin || got > tt.wantMax {
				t.Errorf("NormalizeComplexityCDF(%f) = %f, want between %f and %f",
					tt.avgCognitive, got, tt.wantMin, tt.wantMax)
			}
		})
	}
}

func TestCalculateHotspotScore(t *testing.T) {
	tests := []struct {
		churn      float64
		complexity float64
		want       float64
	}{
		{0.0, 0.0, 0.0},    // Both zero
		{1.0, 0.0, 0.0},    // Complexity zero
		{0.0, 1.0, 0.0},    // Churn zero
		{0.5, 0.5, 0.5},    // sqrt(0.25) = 0.5
		{1.0, 1.0, 1.0},    // sqrt(1.0) = 1.0
		{0.64, 0.64, 0.64}, // sqrt(0.4096) = 0.64
		{0.9, 0.4, 0.6},    // sqrt(0.36) = 0.6
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := CalculateHotspotScore(tt.churn, tt.complexity)
			if got < tt.want-0.01 || got > tt.want+0.01 {
				t.Errorf("CalculateHotspotScore(%f, %f) = %f, want %f",
					tt.churn, tt.complexity, got, tt.want)
			}
		})
	}
}

func TestFileHotspot_Severity(t *testing.T) {
	tests := []struct {
		score    float64
		severity HotspotSeverity
	}{
		{0.7, HotspotSeverityCritical},
		{0.6, HotspotSeverityCritical},
		{0.5, HotspotSeverityHigh},
		{0.4, HotspotSeverityHigh},
		{0.3, HotspotSeverityModerate},
		{0.25, HotspotSeverityModerate},
		{0.2, HotspotSeverityLow},
		{0.1, HotspotSeverityLow},
		{0.0, HotspotSeverityLow},
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			fh := FileHotspot{HotspotScore: tt.score}
			if got := fh.Severity(); got != tt.severity {
				t.Errorf("Severity() for score %f = %s, want %s", tt.score, got, tt.severity)
			}
		})
	}
}

func TestInterpolateHotspotCDF(t *testing.T) {
	// Test linear interpolation between points
	// Using churn percentiles: (2, 0.50) to (3, 0.60)
	got := NormalizeChurnCDF(2) // Should be 0.50
	if got != 0.5 {
		t.Errorf("NormalizeChurnCDF(2) = %f, want 0.5", got)
	}

	// Interpolation at midpoint between 2 and 3
	// (value - 2) / (3 - 2) = 0.5, so percentile = 0.50 + 0.5*(0.60-0.50) = 0.55
	// But we pass int, so we can't test fractional. Test boundary instead.
	got = NormalizeChurnCDF(3) // Should be 0.60
	if got != 0.6 {
		t.Errorf("NormalizeChurnCDF(3) = %f, want 0.6", got)
	}
}

func TestFileHotspot_JSON(t *testing.T) {
	fh := FileHotspot{
		Path:            "test.go",
		HotspotScore:    0.64,
		ChurnScore:      0.8,
		ComplexityScore: 0.8,
		Commits:         10,
		AvgCognitive:    15.0,
		AvgCyclomatic:   8.0,
		TotalFunctions:  5,
	}

	// Verify all fields are accessible
	if fh.Path != "test.go" {
		t.Error("Path not set correctly")
	}
	if fh.HotspotScore != 0.64 {
		t.Error("HotspotScore not set correctly")
	}
	if fh.ChurnScore != 0.8 {
		t.Error("ChurnScore not set correctly")
	}
	if fh.ComplexityScore != 0.8 {
		t.Error("ComplexityScore not set correctly")
	}
}
