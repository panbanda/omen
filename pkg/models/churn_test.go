package models

import (
	"math"
	"testing"
	"time"
)

func TestFileChurnMetrics_CalculateChurnScore(t *testing.T) {
	tests := []struct {
		name           string
		metrics        FileChurnMetrics
		expectedScore  float64
		maxCommits     int
		maxChanges     int
		useDefaultMax  bool
	}{
		{
			name: "zero values",
			metrics: FileChurnMetrics{
				Commits:      0,
				LinesAdded:   0,
				LinesDeleted: 0,
			},
			expectedScore: 0.0,
			useDefaultMax: true,
		},
		{
			name: "half of max commits and changes",
			metrics: FileChurnMetrics{
				Commits:      50,
				LinesAdded:   250,
				LinesDeleted: 250,
			},
			maxCommits:    100,
			maxChanges:    1000,
			expectedScore: 0.5,
		},
		{
			name: "exceeds max commits",
			metrics: FileChurnMetrics{
				Commits:      150,
				LinesAdded:   500,
				LinesDeleted: 500,
			},
			maxCommits:    100,
			maxChanges:    1000,
			expectedScore: 1.0,
		},
		{
			name: "weighted calculation verification",
			metrics: FileChurnMetrics{
				Commits:      30,
				LinesAdded:   200,
				LinesDeleted: 200,
			},
			maxCommits:    100,
			maxChanges:    1000,
			expectedScore: 0.3*0.6 + 0.4*0.4, // 0.18 + 0.16 = 0.34
		},
		{
			name: "only commits",
			metrics: FileChurnMetrics{
				Commits:      80,
				LinesAdded:   0,
				LinesDeleted: 0,
			},
			maxCommits:    100,
			maxChanges:    1000,
			expectedScore: 0.8 * 0.6, // 0.48
		},
		{
			name: "only changes",
			metrics: FileChurnMetrics{
				Commits:      0,
				LinesAdded:   400,
				LinesDeleted: 400,
			},
			maxCommits:    100,
			maxChanges:    1000,
			expectedScore: 0.8 * 0.4, // 0.32
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var score float64
			if tt.useDefaultMax {
				score = tt.metrics.CalculateChurnScore()
			} else {
				score = tt.metrics.CalculateChurnScoreWithMax(tt.maxCommits, tt.maxChanges)
			}

			if math.Abs(score-tt.expectedScore) > 0.001 {
				t.Errorf("CalculateChurnScore() = %v, expected %v", score, tt.expectedScore)
			}

			if tt.metrics.ChurnScore != score {
				t.Errorf("ChurnScore field = %v, expected %v", tt.metrics.ChurnScore, score)
			}
		})
	}
}

func TestFileChurnMetrics_IsHotspot(t *testing.T) {
	tests := []struct {
		name      string
		churnScore float64
		threshold float64
		expected   bool
	}{
		{
			name:       "above threshold",
			churnScore: 0.75,
			threshold:  0.7,
			expected:   true,
		},
		{
			name:       "at threshold",
			churnScore: 0.7,
			threshold:  0.7,
			expected:   true,
		},
		{
			name:       "below threshold",
			churnScore: 0.65,
			threshold:  0.7,
			expected:   false,
		},
		{
			name:       "zero threshold",
			churnScore: 0.01,
			threshold:  0.0,
			expected:   true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			m := &FileChurnMetrics{ChurnScore: tt.churnScore}
			if got := m.IsHotspot(tt.threshold); got != tt.expected {
				t.Errorf("IsHotspot(%v) = %v, expected %v", tt.threshold, got, tt.expected)
			}
		})
	}
}

func TestChurnSummary_CalculateStatistics(t *testing.T) {
	tests := []struct {
		name         string
		files        []FileChurnMetrics
		expectedMean float64
		expectedP50  float64
		expectedP95  float64
	}{
		{
			name:         "empty files",
			files:        []FileChurnMetrics{},
			expectedMean: 0,
			expectedP50:  0,
			expectedP95:  0,
		},
		{
			name: "single file",
			files: []FileChurnMetrics{
				{ChurnScore: 0.5},
			},
			expectedMean: 0.5,
			expectedP50:  0.5,
			expectedP95:  0.5,
		},
		{
			name: "multiple files",
			files: []FileChurnMetrics{
				{ChurnScore: 0.1},
				{ChurnScore: 0.2},
				{ChurnScore: 0.3},
				{ChurnScore: 0.4},
				{ChurnScore: 0.5},
			},
			expectedMean: 0.3,
			expectedP50:  0.3,
			expectedP95:  0.5,
		},
		{
			name: "variance calculation",
			files: []FileChurnMetrics{
				{ChurnScore: 0.0},
				{ChurnScore: 1.0},
			},
			expectedMean: 0.5,
			expectedP50:  1.0,
			expectedP95:  1.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := &ChurnSummary{}
			s.CalculateStatistics(tt.files)

			if math.Abs(s.MeanChurnScore-tt.expectedMean) > 0.001 {
				t.Errorf("MeanChurnScore = %v, expected %v", s.MeanChurnScore, tt.expectedMean)
			}

			if math.Abs(s.P50ChurnScore-tt.expectedP50) > 0.001 {
				t.Errorf("P50ChurnScore = %v, expected %v", s.P50ChurnScore, tt.expectedP50)
			}

			if math.Abs(s.P95ChurnScore-tt.expectedP95) > 0.001 {
				t.Errorf("P95ChurnScore = %v, expected %v", s.P95ChurnScore, tt.expectedP95)
			}

			if len(tt.files) > 1 {
				if s.VarianceChurn < 0 {
					t.Errorf("VarianceChurn = %v, expected >= 0", s.VarianceChurn)
				}
				if s.StdDevChurn < 0 {
					t.Errorf("StdDevChurn = %v, expected >= 0", s.StdDevChurn)
				}
				expectedStdDev := math.Sqrt(s.VarianceChurn)
				if math.Abs(s.StdDevChurn-expectedStdDev) > 0.001 {
					t.Errorf("StdDevChurn = %v, expected %v", s.StdDevChurn, expectedStdDev)
				}
			}
		})
	}
}

func TestPercentileFloat64(t *testing.T) {
	tests := []struct {
		name       string
		sorted     []float64
		percentile int
		expected   float64
	}{
		{
			name:       "empty slice",
			sorted:     []float64{},
			percentile: 50,
			expected:   0.0,
		},
		{
			name:       "50th percentile",
			sorted:     []float64{0.1, 0.2, 0.3, 0.4, 0.5},
			percentile: 50,
			expected:   0.3,
		},
		{
			name:       "95th percentile",
			sorted:     []float64{0.1, 0.2, 0.3, 0.4, 0.5},
			percentile: 95,
			expected:   0.5,
		},
		{
			name:       "0th percentile",
			sorted:     []float64{0.1, 0.2, 0.3},
			percentile: 0,
			expected:   0.1,
		},
		{
			name:       "100th percentile",
			sorted:     []float64{0.1, 0.2, 0.3},
			percentile: 100,
			expected:   0.3,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := percentileFloat64(tt.sorted, tt.percentile)
			if math.Abs(got-tt.expected) > 0.001 {
				t.Errorf("percentileFloat64(%v, %d) = %v, expected %v",
					tt.sorted, tt.percentile, got, tt.expected)
			}
		})
	}
}

func TestNewChurnSummary(t *testing.T) {
	s := NewChurnSummary()

	if s.TopChurnedFiles == nil {
		t.Error("TopChurnedFiles should be initialized")
	}

	if len(s.TopChurnedFiles) != 0 {
		t.Errorf("TopChurnedFiles should be empty, got %d items", len(s.TopChurnedFiles))
	}
}

func TestFileChurnMetrics_TimePeriod(t *testing.T) {
	now := time.Now()
	past := now.Add(-30 * 24 * time.Hour)

	m := FileChurnMetrics{
		FirstCommit: past,
		LastCommit:  now,
	}

	if m.FirstCommit.After(m.LastCommit) {
		t.Error("FirstCommit should be before LastCommit")
	}
}
