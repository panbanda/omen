package models

import (
	"math"
	"sort"
	"time"
)

// FileChurnMetrics represents git churn data for a single file.
type FileChurnMetrics struct {
	Path          string         `json:"path"`
	Commits       int            `json:"commits"`
	UniqueAuthors int            `json:"unique_authors"`
	Authors       map[string]int `json:"authors"` // author email -> commit count
	LinesAdded    int            `json:"lines_added"`
	LinesDeleted  int            `json:"lines_deleted"`
	ChurnScore    float64        `json:"churn_score"` // 0.0-1.0 normalized
	FirstCommit   time.Time      `json:"first_commit"`
	LastCommit    time.Time      `json:"last_commit"`
}

// CalculateChurnScore computes a normalized churn score.
// Uses the same formula as the reference implementation:
// churn_score = (commit_factor * 0.6 + change_factor * 0.4)
func (f *FileChurnMetrics) CalculateChurnScore() float64 {
	return f.CalculateChurnScoreWithMax(100, 1000)
}

// CalculateChurnScoreWithMax computes churn score with explicit max values.
func (f *FileChurnMetrics) CalculateChurnScoreWithMax(maxCommits, maxChanges int) float64 {
	var commitFactor float64
	if maxCommits > 0 {
		commitFactor = float64(f.Commits) / float64(maxCommits)
		if commitFactor > 1.0 {
			commitFactor = 1.0
		}
	}

	var changeFactor float64
	if maxChanges > 0 {
		changeFactor = float64(f.LinesAdded+f.LinesDeleted) / float64(maxChanges)
		if changeFactor > 1.0 {
			changeFactor = 1.0
		}
	}

	score := commitFactor*0.6 + changeFactor*0.4
	if score > 1.0 {
		score = 1.0
	}

	f.ChurnScore = score
	return score
}

// IsHotspot returns true if the file has high churn.
func (f *FileChurnMetrics) IsHotspot(threshold float64) bool {
	return f.ChurnScore >= threshold
}

// ChurnSummary provides aggregate statistics.
type ChurnSummary struct {
	TotalFiles        int      `json:"total_files"`
	TotalCommits      int      `json:"total_commits"`
	TotalLinesAdded   int      `json:"total_lines_added"`
	TotalLinesDeleted int      `json:"total_lines_deleted"`
	UniqueAuthors     int      `json:"unique_authors"`
	AvgCommitsPerFile float64  `json:"avg_commits_per_file"`
	MaxChurnScore     float64  `json:"max_churn_score"`
	TopChurnedFiles   []string `json:"top_churned_files"`
	HotspotFiles      []string `json:"hotspot_files"`
	StableFiles       []string `json:"stable_files"`
	MeanChurnScore    float64  `json:"mean_churn_score"`
	VarianceChurn     float64  `json:"variance_churn_score"`
	StdDevChurn       float64  `json:"stddev_churn_score"`
	P50ChurnScore     float64  `json:"p50_churn_score"`
	P95ChurnScore     float64  `json:"p95_churn_score"`
}

// CalculateStatistics computes mean, variance, standard deviation, and percentiles of churn scores.
func (s *ChurnSummary) CalculateStatistics(files []FileChurnMetrics) {
	if len(files) == 0 {
		return
	}

	// Collect scores for percentile calculation
	scores := make([]float64, len(files))
	var sum float64
	for i, f := range files {
		scores[i] = f.ChurnScore
		sum += f.ChurnScore
	}
	s.MeanChurnScore = sum / float64(len(files))

	// Calculate variance (population variance)
	var varianceSum float64
	for _, f := range files {
		diff := f.ChurnScore - s.MeanChurnScore
		varianceSum += diff * diff
	}
	s.VarianceChurn = varianceSum / float64(len(files))

	// Calculate standard deviation
	s.StdDevChurn = math.Sqrt(s.VarianceChurn)

	// Calculate percentiles (files are already sorted by churn score descending)
	// We need ascending order for percentile calculation
	sortedScores := make([]float64, len(scores))
	copy(sortedScores, scores)
	sort.Float64s(sortedScores)

	s.P50ChurnScore = percentileFloat64(sortedScores, 50)
	s.P95ChurnScore = percentileFloat64(sortedScores, 95)
}

// percentileFloat64 calculates the p-th percentile of a sorted slice.
func percentileFloat64(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// ChurnAnalysis represents the full churn analysis result.
type ChurnAnalysis struct {
	Files    []FileChurnMetrics `json:"files"`
	Summary  ChurnSummary       `json:"summary"`
	Days     int                `json:"days"`
	RepoPath string             `json:"repo_path"`
}

// NewChurnSummary creates an initialized summary.
func NewChurnSummary() ChurnSummary {
	return ChurnSummary{
		TopChurnedFiles: make([]string, 0),
		HotspotFiles:    make([]string, 0),
		StableFiles:     make([]string, 0),
	}
}

// Thresholds for hotspot and stable file detection.
const (
	HotspotThreshold = 0.5
	StableThreshold  = 0.1
)

// IdentifyHotspotAndStableFiles populates HotspotFiles and StableFiles.
// Files must be sorted by ChurnScore descending before calling.
// Hotspots: top 10 files filtered by churn_score > 0.5
// Stable: bottom 10 files filtered by churn_score < 0.1 and commit_count > 0
func (s *ChurnSummary) IdentifyHotspotAndStableFiles(files []FileChurnMetrics) {
	s.HotspotFiles = make([]string, 0)
	s.StableFiles = make([]string, 0)

	// Take top 10 candidates, then filter by threshold
	candidateCount := 10
	if len(files) < candidateCount {
		candidateCount = len(files)
	}
	for i := 0; i < candidateCount; i++ {
		if files[i].ChurnScore > HotspotThreshold {
			s.HotspotFiles = append(s.HotspotFiles, files[i].Path)
		}
	}

	// Take bottom 10 candidates, then filter by threshold
	startIdx := len(files) - 10
	if startIdx < 0 {
		startIdx = 0
	}
	for i := len(files) - 1; i >= startIdx; i-- {
		if files[i].ChurnScore < StableThreshold && files[i].Commits > 0 {
			s.StableFiles = append(s.StableFiles, files[i].Path)
		}
	}
}
