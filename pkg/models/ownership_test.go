package models

import (
	"testing"
	"time"
)

func TestOwnershipAnalysis_CalculateSummary(t *testing.T) {
	tests := []struct {
		name          string
		files         []FileOwnership
		wantBusFactor int
		wantSilos     int
	}{
		{
			name:          "empty",
			files:         []FileOwnership{},
			wantBusFactor: 0,
			wantSilos:     0,
		},
		{
			name: "single silo",
			files: []FileOwnership{
				{
					Path:   "a.go",
					IsSilo: true,
					Contributors: []Contributor{
						{Name: "Alice", LinesOwned: 100},
					},
				},
			},
			wantBusFactor: 1,
			wantSilos:     1,
		},
		{
			name: "shared ownership",
			files: []FileOwnership{
				{
					Path:   "a.go",
					IsSilo: false,
					Contributors: []Contributor{
						{Name: "Alice", LinesOwned: 60},
						{Name: "Bob", LinesOwned: 40},
					},
				},
				{
					Path:   "b.go",
					IsSilo: false,
					Contributors: []Contributor{
						{Name: "Alice", LinesOwned: 30},
						{Name: "Charlie", LinesOwned: 70},
					},
				},
			},
			wantBusFactor: 2, // Alice + Charlie cover 160/200 = 80%
			wantSilos:     0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analysis := &OwnershipAnalysis{
				GeneratedAt: time.Now(),
				Files:       tt.files,
			}
			analysis.CalculateSummary()

			if analysis.Summary.BusFactor != tt.wantBusFactor {
				t.Errorf("BusFactor = %d, want %d", analysis.Summary.BusFactor, tt.wantBusFactor)
			}
			if analysis.Summary.SiloCount != tt.wantSilos {
				t.Errorf("SiloCount = %d, want %d", analysis.Summary.SiloCount, tt.wantSilos)
			}
		})
	}
}

func TestCalculateBusFactor(t *testing.T) {
	tests := []struct {
		name   string
		counts map[string]int
		want   int
	}{
		{
			name:   "empty",
			counts: map[string]int{},
			want:   0,
		},
		{
			name:   "single contributor",
			counts: map[string]int{"Alice": 100},
			want:   1,
		},
		{
			name: "equal contributors",
			counts: map[string]int{
				"Alice": 100,
				"Bob":   100,
			},
			want: 1, // Either one covers 50%
		},
		{
			name: "many small contributors",
			counts: map[string]int{
				"Alice":   100,
				"Bob":     50,
				"Charlie": 50,
				"Dave":    50,
			},
			want: 2, // Alice (100) + Bob (50) = 150/250 = 60%
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := calculateBusFactor(tt.counts)
			if got != tt.want {
				t.Errorf("calculateBusFactor() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestCalculateConcentration(t *testing.T) {
	tests := []struct {
		name         string
		contributors []Contributor
		want         float64
	}{
		{
			name:         "empty",
			contributors: []Contributor{},
			want:         0,
		},
		{
			name: "single owner",
			contributors: []Contributor{
				{Percentage: 100},
			},
			want: 1.0,
		},
		{
			name: "two equal owners",
			contributors: []Contributor{
				{Percentage: 50},
				{Percentage: 50},
			},
			want: 0.5,
		},
		{
			name: "dominant owner",
			contributors: []Contributor{
				{Percentage: 80},
				{Percentage: 20},
			},
			want: 0.8,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := CalculateConcentration(tt.contributors)
			if got != tt.want {
				t.Errorf("CalculateConcentration() = %f, want %f", got, tt.want)
			}
		})
	}
}

func TestGetTopContributors(t *testing.T) {
	counts := map[string]int{
		"Alice":   100,
		"Bob":     80,
		"Charlie": 60,
		"Dave":    40,
		"Eve":     20,
	}

	top3 := getTopContributors(counts, 3)
	if len(top3) != 3 {
		t.Fatalf("Expected 3 contributors, got %d", len(top3))
	}

	// Should be in order: Alice, Bob, Charlie
	expected := []string{"Alice", "Bob", "Charlie"}
	for i, name := range expected {
		if top3[i] != name {
			t.Errorf("Top contributor %d = %q, want %q", i, top3[i], name)
		}
	}
}

func TestFileOwnership_Fields(t *testing.T) {
	fo := FileOwnership{
		Path:             "/test/file.go",
		PrimaryOwner:     "Alice",
		OwnershipPercent: 75.5,
		Concentration:    0.755,
		TotalLines:       200,
		IsSilo:           false,
		Contributors: []Contributor{
			{Name: "Alice", Email: "alice@example.com", LinesOwned: 151, Percentage: 75.5},
			{Name: "Bob", Email: "bob@example.com", LinesOwned: 49, Percentage: 24.5},
		},
	}

	if fo.Path != "/test/file.go" {
		t.Errorf("Path = %q, want %q", fo.Path, "/test/file.go")
	}
	if fo.PrimaryOwner != "Alice" {
		t.Errorf("PrimaryOwner = %q, want %q", fo.PrimaryOwner, "Alice")
	}
	if fo.IsSilo {
		t.Error("IsSilo = true, want false")
	}
	if len(fo.Contributors) != 2 {
		t.Errorf("Contributors count = %d, want 2", len(fo.Contributors))
	}
}
