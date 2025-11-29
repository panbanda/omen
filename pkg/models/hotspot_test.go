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
			wantHotspots: 2, // 0.9 and 0.6 are >= 0.5
			wantMax:      0.9,
			wantAvg:      0.5, // (0.9 + 0.6 + 0.4 + 0.1) / 4 = 0.5
		},
		{
			name: "all below threshold",
			files: []FileHotspot{
				{Path: "a.go", HotspotScore: 0.4},
				{Path: "b.go", HotspotScore: 0.3},
				{Path: "c.go", HotspotScore: 0.2},
			},
			wantTotal:    3,
			wantHotspots: 0,
			wantMax:      0.4,
			wantAvg:      0.3,
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
	if DefaultHotspotScoreThreshold != 0.5 {
		t.Errorf("DefaultHotspotScoreThreshold = %f, want 0.5", DefaultHotspotScoreThreshold)
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
