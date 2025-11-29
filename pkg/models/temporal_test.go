package models

import (
	"testing"
	"time"
)

func TestTemporalCouplingAnalysis_CalculateSummary(t *testing.T) {
	tests := []struct {
		name       string
		couplings  []FileCoupling
		totalFiles int
		wantTotal  int
		wantStrong int
		wantMax    float64
	}{
		{
			name:       "empty",
			couplings:  []FileCoupling{},
			totalFiles: 10,
			wantTotal:  0,
			wantStrong: 0,
			wantMax:    0,
		},
		{
			name: "single coupling",
			couplings: []FileCoupling{
				{FileA: "a.go", FileB: "b.go", CouplingStrength: 0.8},
			},
			totalFiles: 5,
			wantTotal:  1,
			wantStrong: 1,
			wantMax:    0.8,
		},
		{
			name: "multiple couplings sorted descending",
			couplings: []FileCoupling{
				{FileA: "a.go", FileB: "b.go", CouplingStrength: 0.9},
				{FileA: "c.go", FileB: "d.go", CouplingStrength: 0.6},
				{FileA: "e.go", FileB: "f.go", CouplingStrength: 0.3},
			},
			totalFiles: 6,
			wantTotal:  3,
			wantStrong: 2, // 0.9 and 0.6 are >= 0.5
			wantMax:    0.9,
		},
		{
			name: "all weak couplings",
			couplings: []FileCoupling{
				{FileA: "a.go", FileB: "b.go", CouplingStrength: 0.4},
				{FileA: "c.go", FileB: "d.go", CouplingStrength: 0.3},
			},
			totalFiles: 4,
			wantTotal:  2,
			wantStrong: 0,
			wantMax:    0.4,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analysis := &TemporalCouplingAnalysis{
				GeneratedAt: time.Now(),
				PeriodDays:  30,
				Couplings:   tt.couplings,
			}
			analysis.CalculateSummary(tt.totalFiles)

			if analysis.Summary.TotalCouplings != tt.wantTotal {
				t.Errorf("TotalCouplings = %d, want %d", analysis.Summary.TotalCouplings, tt.wantTotal)
			}
			if analysis.Summary.StrongCouplings != tt.wantStrong {
				t.Errorf("StrongCouplings = %d, want %d", analysis.Summary.StrongCouplings, tt.wantStrong)
			}
			if analysis.Summary.MaxCouplingStrength != tt.wantMax {
				t.Errorf("MaxCouplingStrength = %f, want %f", analysis.Summary.MaxCouplingStrength, tt.wantMax)
			}
			if analysis.Summary.TotalFilesAnalyzed != tt.totalFiles {
				t.Errorf("TotalFilesAnalyzed = %d, want %d", analysis.Summary.TotalFilesAnalyzed, tt.totalFiles)
			}
		})
	}
}

func TestCalculateCouplingStrength(t *testing.T) {
	tests := []struct {
		name      string
		cochanges int
		commitsA  int
		commitsB  int
		want      float64
	}{
		{
			name:      "zero commits",
			cochanges: 0,
			commitsA:  0,
			commitsB:  0,
			want:      0,
		},
		{
			name:      "perfect coupling",
			cochanges: 10,
			commitsA:  10,
			commitsB:  10,
			want:      1.0,
		},
		{
			name:      "half coupling",
			cochanges: 5,
			commitsA:  10,
			commitsB:  10,
			want:      0.5,
		},
		{
			name:      "asymmetric commits",
			cochanges: 5,
			commitsA:  10,
			commitsB:  5,
			want:      0.5, // 5/10 = 0.5
		},
		{
			name:      "capped at 1.0",
			cochanges: 15,
			commitsA:  10,
			commitsB:  5,
			want:      1.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateCouplingStrength(tt.cochanges, tt.commitsA, tt.commitsB)
			if got != tt.want {
				t.Errorf("CalculateCouplingStrength(%d, %d, %d) = %f, want %f",
					tt.cochanges, tt.commitsA, tt.commitsB, got, tt.want)
			}
		})
	}
}

func TestDefaultMinCochanges(t *testing.T) {
	if DefaultMinCochanges != 3 {
		t.Errorf("DefaultMinCochanges = %d, want 3", DefaultMinCochanges)
	}
}

func TestStrongCouplingThreshold(t *testing.T) {
	if StrongCouplingThreshold != 0.5 {
		t.Errorf("StrongCouplingThreshold = %f, want 0.5", StrongCouplingThreshold)
	}
}

func TestFileCoupling_Fields(t *testing.T) {
	fc := FileCoupling{
		FileA:            "src/main.go",
		FileB:            "src/util.go",
		CochangeCount:    15,
		CouplingStrength: 0.75,
		CommitsA:         20,
		CommitsB:         18,
	}

	if fc.FileA != "src/main.go" {
		t.Errorf("FileA = %q, want %q", fc.FileA, "src/main.go")
	}
	if fc.CochangeCount != 15 {
		t.Errorf("CochangeCount = %d, want %d", fc.CochangeCount, 15)
	}
	if fc.CouplingStrength != 0.75 {
		t.Errorf("CouplingStrength = %f, want %f", fc.CouplingStrength, 0.75)
	}
}
