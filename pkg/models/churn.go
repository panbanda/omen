package models

import (
	"math"
	"sort"
	"time"
)

// FileChurnMetrics represents git churn data for a single file.
type FileChurnMetrics struct {
	Path          string         `json:"path"`
	RelativePath  string         `json:"relative_path"`
	Commits       int            `json:"commit_count"`
	UniqueAuthors []string       `json:"unique_authors"`
	AuthorCounts  map[string]int `json:"-"` // internal: author name -> commit count
	LinesAdded    int            `json:"additions"`
	LinesDeleted  int            `json:"deletions"`
	ChurnScore    float64        `json:"churn_score"` // 0.0-1.0 normalized
	FirstCommit   time.Time      `json:"first_seen"`
	LastCommit    time.Time      `json:"last_modified"`

	// Relative churn metrics (research-backed - Nagappan & Ball 2005)
	TotalLOC        int     `json:"total_loc,omitempty"`        // Current lines of code in file
	LOCReadError    bool    `json:"loc_read_error,omitempty"`   // True if file could not be read for LOC count
	RelativeChurn   float64 `json:"relative_churn,omitempty"`   // (LinesAdded + LinesDeleted) / TotalLOC
	ChurnRate       float64 `json:"churn_rate,omitempty"`       // RelativeChurn / DaysSinceFirstCommit
	ChangeFrequency float64 `json:"change_frequency,omitempty"` // Commits / DaysSinceFirstCommit
	DaysActive      int     `json:"days_active,omitempty"`      // Days between first and last commit
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

// CalculateRelativeChurn computes relative churn metrics.
// These metrics are research-backed per Nagappan & Ball (2005):
// "Use of relative code churn measures to predict system defect density"
// Relative churn discriminates fault-prone files with 89% accuracy.
func (f *FileChurnMetrics) CalculateRelativeChurn(now time.Time) {
	// Calculate days active
	if !f.FirstCommit.IsZero() && !f.LastCommit.IsZero() {
		f.DaysActive = int(f.LastCommit.Sub(f.FirstCommit).Hours() / 24)
		if f.DaysActive < 1 {
			f.DaysActive = 1 // Minimum 1 day
		}
	}

	// Calculate relative churn: (LinesAdded + LinesDeleted) / TotalLOC
	if f.TotalLOC > 0 {
		f.RelativeChurn = float64(f.LinesAdded+f.LinesDeleted) / float64(f.TotalLOC)
	}

	// Calculate churn rate: RelativeChurn / DaysActive
	if f.DaysActive > 0 {
		f.ChurnRate = f.RelativeChurn / float64(f.DaysActive)
	}

	// Calculate change frequency: Commits / DaysActive
	if f.DaysActive > 0 {
		f.ChangeFrequency = float64(f.Commits) / float64(f.DaysActive)
	}
}

// ChurnSummary provides aggregate statistics.
type ChurnSummary struct {
	// Required fields matching pmat
	TotalCommits        int            `json:"total_commits"`
	TotalFilesChanged   int            `json:"total_files_changed"`
	HotspotFiles        []string       `json:"hotspot_files"`
	StableFiles         []string       `json:"stable_files"`
	AuthorContributions map[string]int `json:"author_contributions"`
	MeanChurnScore      float64        `json:"mean_churn_score"`
	VarianceChurnScore  float64        `json:"variance_churn_score"`
	StdDevChurnScore    float64        `json:"stddev_churn_score"`
	// Additional metrics not in pmat
	TotalAdditions    int     `json:"total_additions,omitempty"`
	TotalDeletions    int     `json:"total_deletions,omitempty"`
	AvgCommitsPerFile float64 `json:"avg_commits_per_file,omitempty"`
	MaxChurnScore     float64 `json:"max_churn_score,omitempty"`
	P50ChurnScore     float64 `json:"p50_churn_score,omitempty"`
	P95ChurnScore     float64 `json:"p95_churn_score,omitempty"`
}

// CalculateStatistics computes mean, variance, standard deviation, and percentiles.
func (s *ChurnSummary) CalculateStatistics(files []FileChurnMetrics) {
	if len(files) == 0 {
		return
	}

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
	s.VarianceChurnScore = varianceSum / float64(len(files))

	// Calculate standard deviation
	s.StdDevChurnScore = math.Sqrt(s.VarianceChurnScore)

	// Calculate percentiles
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
	GeneratedAt    time.Time          `json:"generated_at"`
	PeriodDays     int                `json:"period_days"`
	RepositoryRoot string             `json:"repository_root"`
	Files          []FileChurnMetrics `json:"files"`
	Summary        ChurnSummary       `json:"summary"`
}

// NewChurnSummary creates an initialized summary.
func NewChurnSummary() ChurnSummary {
	return ChurnSummary{
		HotspotFiles:        make([]string, 0),
		StableFiles:         make([]string, 0),
		AuthorContributions: make(map[string]int),
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
