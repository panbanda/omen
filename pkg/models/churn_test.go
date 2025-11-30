package models

import (
	"math"
	"testing"
	"time"
)

func TestFileChurnMetrics_CalculateChurnScore(t *testing.T) {
	tests := []struct {
		name          string
		metrics       FileChurnMetrics
		expectedScore float64
		maxCommits    int
		maxChanges    int
		useDefaultMax bool
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
		{
			// Mirrors PMAT test_churn_score_calculation
			// Expected: (10/20)*0.6 + (150/300)*0.4 = 0.5
			name: "pmat reference calculation",
			metrics: FileChurnMetrics{
				Commits:      10,
				LinesAdded:   100,
				LinesDeleted: 50,
			},
			maxCommits:    20,
			maxChanges:    300,
			expectedScore: 0.5, // (10/20)*0.6 + (150/300)*0.4 = 0.3 + 0.2 = 0.5
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
		name       string
		churnScore float64
		threshold  float64
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
				if s.VarianceChurnScore < 0 {
					t.Errorf("VarianceChurnScore = %v, expected >= 0", s.VarianceChurnScore)
				}
				if s.StdDevChurnScore < 0 {
					t.Errorf("StdDevChurnScore = %v, expected >= 0", s.StdDevChurnScore)
				}
				expectedStdDev := math.Sqrt(s.VarianceChurnScore)
				if math.Abs(s.StdDevChurnScore-expectedStdDev) > 0.001 {
					t.Errorf("StdDevChurnScore = %v, expected %v", s.StdDevChurnScore, expectedStdDev)
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

	if s.HotspotFiles == nil {
		t.Error("HotspotFiles should be initialized")
	}
	if len(s.HotspotFiles) != 0 {
		t.Errorf("HotspotFiles should be empty, got %d items", len(s.HotspotFiles))
	}

	if s.AuthorContributions == nil {
		t.Error("AuthorContributions should be initialized")
	}
	if len(s.AuthorContributions) != 0 {
		t.Errorf("AuthorContributions should be empty, got %d items", len(s.AuthorContributions))
	}

	if s.StableFiles == nil {
		t.Error("StableFiles should be initialized")
	}
	if len(s.StableFiles) != 0 {
		t.Errorf("StableFiles should be empty, got %d items", len(s.StableFiles))
	}
}

func TestChurnSummary_IdentifyHotspotAndStableFiles(t *testing.T) {
	tests := []struct {
		name            string
		files           []FileChurnMetrics
		expectedHotspot []string
		expectedStable  []string
	}{
		{
			name:            "empty files",
			files:           []FileChurnMetrics{},
			expectedHotspot: []string{},
			expectedStable:  []string{},
		},
		{
			name: "hotspots only",
			files: []FileChurnMetrics{
				{Path: "hot1.go", ChurnScore: 0.9, Commits: 50},
				{Path: "hot2.go", ChurnScore: 0.7, Commits: 30},
				{Path: "hot3.go", ChurnScore: 0.6, Commits: 20},
				{Path: "mid.go", ChurnScore: 0.4, Commits: 10},
			},
			expectedHotspot: []string{"hot1.go", "hot2.go", "hot3.go"},
			expectedStable:  []string{},
		},
		{
			name: "stable only",
			files: []FileChurnMetrics{
				{Path: "mid.go", ChurnScore: 0.3, Commits: 10},
				{Path: "stable1.go", ChurnScore: 0.05, Commits: 2},
				{Path: "stable2.go", ChurnScore: 0.02, Commits: 1},
			},
			expectedHotspot: []string{},
			expectedStable:  []string{"stable2.go", "stable1.go"},
		},
		{
			name: "both hotspots and stable",
			files: []FileChurnMetrics{
				{Path: "hot.go", ChurnScore: 0.8, Commits: 40},
				{Path: "mid.go", ChurnScore: 0.3, Commits: 10},
				{Path: "stable.go", ChurnScore: 0.05, Commits: 3},
			},
			expectedHotspot: []string{"hot.go"},
			expectedStable:  []string{"stable.go"},
		},
		{
			name: "stable file with zero commits excluded",
			files: []FileChurnMetrics{
				{Path: "mid.go", ChurnScore: 0.3, Commits: 10},
				{Path: "stable.go", ChurnScore: 0.05, Commits: 1},
				{Path: "zero.go", ChurnScore: 0.0, Commits: 0},
			},
			expectedHotspot: []string{},
			expectedStable:  []string{"stable.go"},
		},
		{
			name: "take 10 then filter hotspots",
			files: func() []FileChurnMetrics {
				// 15 files, all above threshold, but only first 10 are candidates
				files := make([]FileChurnMetrics, 15)
				for i := 0; i < 15; i++ {
					files[i] = FileChurnMetrics{
						Path:       "hot" + string(rune('a'+i)) + ".go",
						ChurnScore: 0.9 - float64(i)*0.01,
						Commits:    50 - i,
					}
				}
				return files
			}(),
			// Only first 10 candidates are checked, all pass threshold
			expectedHotspot: []string{"hota.go", "hotb.go", "hotc.go", "hotd.go", "hote.go", "hotf.go", "hotg.go", "hoth.go", "hoti.go", "hotj.go"},
			expectedStable:  []string{},
		},
		{
			name: "take 10 candidates but fewer pass threshold",
			files: func() []FileChurnMetrics {
				// 15 files sorted descending
				// Scores: 0.55, 0.52, 0.49, 0.46... (step of 0.03)
				// Only first 2 are > 0.5 threshold
				files := make([]FileChurnMetrics, 15)
				for i := 0; i < 15; i++ {
					files[i] = FileChurnMetrics{
						Path:       "file" + string(rune('a'+i)) + ".go",
						ChurnScore: 0.55 - float64(i)*0.03, // 0.55, 0.52, 0.49, 0.46... down to 0.13
						Commits:    50 - i,
					}
				}
				return files
			}(),
			// Only first 2 pass threshold (0.55 > 0.5, 0.52 > 0.5, 0.49 not > 0.5)
			// Bottom files have scores >= 0.13 so none are stable (< 0.1)
			expectedHotspot: []string{"filea.go", "fileb.go"},
			expectedStable:  []string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			s := &ChurnSummary{}
			s.IdentifyHotspotAndStableFiles(tt.files)

			if len(s.HotspotFiles) != len(tt.expectedHotspot) {
				t.Errorf("HotspotFiles count = %d, expected %d", len(s.HotspotFiles), len(tt.expectedHotspot))
			}
			for i, path := range tt.expectedHotspot {
				if i < len(s.HotspotFiles) && s.HotspotFiles[i] != path {
					t.Errorf("HotspotFiles[%d] = %s, expected %s", i, s.HotspotFiles[i], path)
				}
			}

			if len(s.StableFiles) != len(tt.expectedStable) {
				t.Errorf("StableFiles count = %d, expected %d", len(s.StableFiles), len(tt.expectedStable))
			}
			for i, path := range tt.expectedStable {
				if i < len(s.StableFiles) && s.StableFiles[i] != path {
					t.Errorf("StableFiles[%d] = %s, expected %s", i, s.StableFiles[i], path)
				}
			}
		})
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

// TestChurnScoreEdgeCases mirrors PMAT test_churn_score_edge_cases
func TestChurnScoreEdgeCases(t *testing.T) {
	t.Run("zero max values", func(t *testing.T) {
		m := FileChurnMetrics{
			Path:         "test.go",
			Commits:      0,
			LinesAdded:   0,
			LinesDeleted: 0,
		}
		m.CalculateChurnScoreWithMax(0, 0)
		if m.ChurnScore != 0.0 {
			t.Errorf("ChurnScore = %v, expected 0.0", m.ChurnScore)
		}
	})

	t.Run("non-zero values with zero max", func(t *testing.T) {
		m := FileChurnMetrics{
			Path:         "test.go",
			Commits:      5,
			LinesAdded:   100,
			LinesDeleted: 50,
		}
		m.CalculateChurnScoreWithMax(0, 0)
		if m.ChurnScore != 0.0 {
			t.Errorf("ChurnScore = %v, expected 0.0 when max is 0", m.ChurnScore)
		}
	})

	t.Run("max values equal to file values", func(t *testing.T) {
		m := FileChurnMetrics{
			Path:         "test.go",
			Commits:      20,
			LinesAdded:   150,
			LinesDeleted: 150,
		}
		m.CalculateChurnScoreWithMax(20, 300)
		if m.ChurnScore != 1.0 {
			t.Errorf("ChurnScore = %v, expected 1.0", m.ChurnScore)
		}
	})
}

// TestChurnSummaryWithData mirrors PMAT test_churn_summary_with_data
func TestChurnSummaryWithData(t *testing.T) {
	// Create test data similar to PMAT's create_test_analysis
	files := []FileChurnMetrics{
		{
			Path:          "src/main.go",
			RelativePath:  "src/main.go",
			Commits:       25,
			LinesAdded:    300,
			LinesDeleted:  150,
			ChurnScore:    0.85,
			UniqueAuthors: []string{"alice", "bob"},
			AuthorCounts:  map[string]int{"alice": 15, "bob": 10},
		},
		{
			Path:          "src/lib.go",
			RelativePath:  "src/lib.go",
			Commits:       5,
			LinesAdded:    50,
			LinesDeleted:  10,
			ChurnScore:    0.15,
			UniqueAuthors: []string{"alice"},
			AuthorCounts:  map[string]int{"alice": 5},
		},
	}

	summary := ChurnSummary{
		TotalFilesChanged:   2,
		TotalCommits:        30,
		TotalAdditions:      350,
		TotalDeletions:      160,
		AuthorContributions: map[string]int{"alice": 20, "bob": 10},
	}
	summary.IdentifyHotspotAndStableFiles(files)

	if summary.TotalCommits != 30 {
		t.Errorf("TotalCommits = %d, expected 30", summary.TotalCommits)
	}

	// First file (0.85) is a hotspot (> 0.5)
	if len(summary.HotspotFiles) != 1 {
		t.Errorf("HotspotFiles count = %d, expected 1", len(summary.HotspotFiles))
	}
	if len(summary.HotspotFiles) > 0 && summary.HotspotFiles[0] != "src/main.go" {
		t.Errorf("HotspotFiles[0] = %s, expected src/main.go", summary.HotspotFiles[0])
	}

	// Second file (0.15) is NOT stable (>= 0.1 threshold)
	if len(summary.StableFiles) != 0 {
		t.Errorf("StableFiles count = %d, expected 0 (0.15 >= 0.1)", len(summary.StableFiles))
	}
}

// TestChurnAnalysisCreation mirrors PMAT test_code_churn_analysis_creation
func TestChurnAnalysisCreation(t *testing.T) {
	analysis := ChurnAnalysis{
		Files:          []FileChurnMetrics{},
		Summary:        NewChurnSummary(),
		PeriodDays:     30,
		RepositoryRoot: "/test/repo",
	}

	if analysis.PeriodDays != 30 {
		t.Errorf("PeriodDays = %d, expected 30", analysis.PeriodDays)
	}

	if analysis.Summary.TotalCommits != 0 {
		t.Errorf("TotalCommits = %d, expected 0", analysis.Summary.TotalCommits)
	}

	if analysis.Summary.TotalFilesChanged != 0 {
		t.Errorf("TotalFilesChanged = %d, expected 0", analysis.Summary.TotalFilesChanged)
	}
}

// TestStatisticsCalculation verifies mean/variance/stddev match PMAT formula
func TestStatisticsCalculation(t *testing.T) {
	// Two files with scores 0.85 and 0.15
	// Mean = (0.85 + 0.15) / 2 = 0.5
	// Variance = ((0.85-0.5)^2 + (0.15-0.5)^2) / 2 = (0.1225 + 0.1225) / 2 = 0.1225
	// StdDev = sqrt(0.1225) = 0.35
	files := []FileChurnMetrics{
		{ChurnScore: 0.85},
		{ChurnScore: 0.15},
	}

	summary := &ChurnSummary{}
	summary.CalculateStatistics(files)

	if math.Abs(summary.MeanChurnScore-0.5) > 0.001 {
		t.Errorf("MeanChurnScore = %v, expected 0.5", summary.MeanChurnScore)
	}

	if math.Abs(summary.VarianceChurnScore-0.1225) > 0.001 {
		t.Errorf("VarianceChurnScore = %v, expected 0.1225", summary.VarianceChurnScore)
	}

	if math.Abs(summary.StdDevChurnScore-0.35) > 0.001 {
		t.Errorf("StdDevChurnScore = %v, expected 0.35", summary.StdDevChurnScore)
	}
}

// TestEmptyAnalysis mirrors PMAT test_empty_repository scenario
func TestEmptyAnalysis(t *testing.T) {
	files := []FileChurnMetrics{}

	summary := &ChurnSummary{}
	summary.CalculateStatistics(files)
	summary.IdentifyHotspotAndStableFiles(files)

	if len(summary.HotspotFiles) != 0 {
		t.Errorf("HotspotFiles should be empty, got %d", len(summary.HotspotFiles))
	}

	if len(summary.StableFiles) != 0 {
		t.Errorf("StableFiles should be empty, got %d", len(summary.StableFiles))
	}

	if summary.MeanChurnScore != 0 {
		t.Errorf("MeanChurnScore = %v, expected 0", summary.MeanChurnScore)
	}

	if summary.VarianceChurnScore != 0 {
		t.Errorf("VarianceChurnScore = %v, expected 0", summary.VarianceChurnScore)
	}

	if summary.StdDevChurnScore != 0 {
		t.Errorf("StdDevChurnScore = %v, expected 0", summary.StdDevChurnScore)
	}
}

// TestThresholdConstants verifies thresholds match PMAT
func TestThresholdConstants(t *testing.T) {
	if HotspotThreshold != 0.5 {
		t.Errorf("HotspotThreshold = %v, expected 0.5", HotspotThreshold)
	}

	if StableThreshold != 0.1 {
		t.Errorf("StableThreshold = %v, expected 0.1", StableThreshold)
	}
}

// TestCalculateRelativeChurn verifies relative churn metrics (Nagappan & Ball 2005)
func TestCalculateRelativeChurn(t *testing.T) {
	now := time.Now()

	tests := []struct {
		name                   string
		metrics                FileChurnMetrics
		expectedRelativeChurn  float64
		expectedChurnRate      float64
		expectedChangeFreq     float64
		expectedDaysActive     int
	}{
		{
			name: "standard case",
			metrics: FileChurnMetrics{
				LinesAdded:   100,
				LinesDeleted: 50,
				TotalLOC:     500,
				Commits:      10,
				FirstCommit:  now.AddDate(0, 0, -10),
				LastCommit:   now,
			},
			expectedRelativeChurn: 0.3,   // (100 + 50) / 500
			expectedChurnRate:     0.03,  // 0.3 / 10
			expectedChangeFreq:    1.0,   // 10 / 10
			expectedDaysActive:    10,
		},
		{
			name: "zero LOC",
			metrics: FileChurnMetrics{
				LinesAdded:   50,
				LinesDeleted: 30,
				TotalLOC:     0,
				Commits:      5,
				FirstCommit:  now.AddDate(0, 0, -5),
				LastCommit:   now,
			},
			expectedRelativeChurn: 0.0, // Cannot calculate without LOC
			expectedChurnRate:     0.0,
			expectedChangeFreq:    1.0, // 5 / 5
			expectedDaysActive:    5,
		},
		{
			name: "same day commits",
			metrics: FileChurnMetrics{
				LinesAdded:   20,
				LinesDeleted: 10,
				TotalLOC:     100,
				Commits:      3,
				FirstCommit:  now,
				LastCommit:   now,
			},
			expectedRelativeChurn: 0.3,  // (20 + 10) / 100
			expectedChurnRate:     0.3,  // 0.3 / 1 (minimum 1 day)
			expectedChangeFreq:    3.0,  // 3 / 1
			expectedDaysActive:    1,    // Minimum
		},
		{
			name: "high churn file",
			metrics: FileChurnMetrics{
				LinesAdded:   500,
				LinesDeleted: 300,
				TotalLOC:     200, // More churn than current size
				Commits:      50,
				FirstCommit:  now.AddDate(0, 0, -30),
				LastCommit:   now,
			},
			expectedRelativeChurn: 4.0,   // (500 + 300) / 200
			expectedChurnRate:     4.0/30,
			expectedChangeFreq:    50.0/30,
			expectedDaysActive:    30,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			m := tt.metrics
			m.CalculateRelativeChurn(now)

			if math.Abs(m.RelativeChurn-tt.expectedRelativeChurn) > 0.001 {
				t.Errorf("RelativeChurn = %v, expected %v", m.RelativeChurn, tt.expectedRelativeChurn)
			}

			if math.Abs(m.ChurnRate-tt.expectedChurnRate) > 0.001 {
				t.Errorf("ChurnRate = %v, expected %v", m.ChurnRate, tt.expectedChurnRate)
			}

			if math.Abs(m.ChangeFrequency-tt.expectedChangeFreq) > 0.001 {
				t.Errorf("ChangeFrequency = %v, expected %v", m.ChangeFrequency, tt.expectedChangeFreq)
			}

			if m.DaysActive != tt.expectedDaysActive {
				t.Errorf("DaysActive = %d, expected %d", m.DaysActive, tt.expectedDaysActive)
			}
		})
	}
}

// TestCalculateRelativeChurn_ZeroTime verifies handling of zero timestamps
func TestCalculateRelativeChurn_ZeroTime(t *testing.T) {
	now := time.Now()
	m := FileChurnMetrics{
		LinesAdded:   100,
		LinesDeleted: 50,
		TotalLOC:     500,
		Commits:      10,
		// Zero times - not set
	}
	m.CalculateRelativeChurn(now)

	if m.DaysActive != 0 {
		t.Errorf("DaysActive = %d, expected 0 with zero timestamps", m.DaysActive)
	}

	// RelativeChurn should still calculate since it only depends on LOC
	if math.Abs(m.RelativeChurn-0.3) > 0.001 {
		t.Errorf("RelativeChurn = %v, expected 0.3", m.RelativeChurn)
	}
}
