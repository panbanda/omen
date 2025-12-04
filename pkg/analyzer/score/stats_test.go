package score

import (
	"math"
	"testing"
	"time"
)

func TestComputeTrendStats(t *testing.T) {
	tests := []struct {
		name      string
		points    []TrendPoint
		wantSlope float64
		wantRSq   float64
		wantCorr  float64
		slopeTol  float64
		rSqMin    float64
	}{
		{
			name:   "empty points",
			points: []TrendPoint{},
		},
		{
			name: "single point",
			points: []TrendPoint{
				{Score: 70},
			},
		},
		{
			name: "two points increasing",
			points: []TrendPoint{
				{Score: 60},
				{Score: 80},
			},
			wantSlope: 20.0,
			wantRSq:   1.0,
			wantCorr:  1.0,
			slopeTol:  0.01,
			rSqMin:    0.99,
		},
		{
			name: "two points decreasing",
			points: []TrendPoint{
				{Score: 80},
				{Score: 60},
			},
			wantSlope: -20.0,
			wantRSq:   1.0,
			wantCorr:  -1.0,
			slopeTol:  0.01,
			rSqMin:    0.99,
		},
		{
			name: "linear progression",
			points: []TrendPoint{
				{Score: 50},
				{Score: 55},
				{Score: 60},
				{Score: 65},
				{Score: 70},
			},
			wantSlope: 5.0,
			wantRSq:   1.0,
			wantCorr:  1.0,
			slopeTol:  0.01,
			rSqMin:    0.99,
		},
		{
			name: "noisy but trending up",
			points: []TrendPoint{
				{Score: 50},
				{Score: 58},
				{Score: 55},
				{Score: 65},
				{Score: 62},
				{Score: 70},
			},
			wantSlope: 3.5,  // approximate
			wantRSq:   0.85, // not perfect fit
			wantCorr:  0.92,
			slopeTol:  0.5,
			rSqMin:    0.80,
		},
		{
			name: "stable scores",
			points: []TrendPoint{
				{Score: 70},
				{Score: 70},
				{Score: 70},
				{Score: 70},
			},
			wantSlope: 0.0,
			slopeTol:  0.01,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			stats := ComputeTrendStats(tt.points)

			if len(tt.points) < 2 {
				if stats.Slope != 0 || stats.RSquared != 0 {
					t.Errorf("expected zero stats for <2 points, got slope=%v, r2=%v",
						stats.Slope, stats.RSquared)
				}
				return
			}

			if tt.slopeTol > 0 && math.Abs(stats.Slope-tt.wantSlope) > tt.slopeTol {
				t.Errorf("slope = %v, want %v (tol %v)", stats.Slope, tt.wantSlope, tt.slopeTol)
			}

			if tt.rSqMin > 0 && stats.RSquared < tt.rSqMin {
				t.Errorf("r² = %v, want >= %v", stats.RSquared, tt.rSqMin)
			}
		})
	}
}

func TestParseSince(t *testing.T) {
	tests := []struct {
		input   string
		want    time.Duration
		wantErr bool
	}{
		{"1y", 365 * 24 * time.Hour, false},
		{"2y", 2 * 365 * 24 * time.Hour, false},
		{"6m", 6 * 30 * 24 * time.Hour, false},
		{"3m", 3 * 30 * 24 * time.Hour, false},
		{"4w", 4 * 7 * 24 * time.Hour, false},
		{"30d", 30 * 24 * time.Hour, false},
		{"invalid", 0, true},
		{"1x", 0, true},
		{"m", 0, true},
		{"", 0, true},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got, err := ParseSince(tt.input)
			if (err != nil) != tt.wantErr {
				t.Errorf("ParseSince(%q) error = %v, wantErr %v", tt.input, err, tt.wantErr)
				return
			}
			if got != tt.want {
				t.Errorf("ParseSince(%q) = %v, want %v", tt.input, got, tt.want)
			}
		})
	}
}
