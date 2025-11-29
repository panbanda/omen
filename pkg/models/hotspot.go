package models

import (
	"sort"
	"time"
)

// FileHotspot represents hotspot metrics for a single file.
type FileHotspot struct {
	Path            string  `json:"path"`
	HotspotScore    float64 `json:"hotspot_score"`    // 0-1, churn × complexity
	ChurnScore      float64 `json:"churn_score"`      // 0-1, normalized
	ComplexityScore float64 `json:"complexity_score"` // 0-1, normalized
	Commits         int     `json:"commits"`
	AvgCognitive    float64 `json:"avg_cognitive"`
	AvgCyclomatic   float64 `json:"avg_cyclomatic"`
	TotalFunctions  int     `json:"total_functions"`
}

// HotspotSummary provides aggregate statistics for hotspot analysis.
type HotspotSummary struct {
	TotalFiles      int     `json:"total_files"`
	HotspotCount    int     `json:"hotspot_count"` // Files above threshold
	MaxHotspotScore float64 `json:"max_hotspot_score"`
	AvgHotspotScore float64 `json:"avg_hotspot_score"`
	P50HotspotScore float64 `json:"p50_hotspot_score"`
	P90HotspotScore float64 `json:"p90_hotspot_score"`
}

// HotspotAnalysis represents the full hotspot analysis result.
type HotspotAnalysis struct {
	GeneratedAt time.Time      `json:"generated_at"`
	PeriodDays  int            `json:"period_days"`
	Files       []FileHotspot  `json:"files"`
	Summary     HotspotSummary `json:"summary"`
}

// DefaultHotspotScoreThreshold is the default threshold for considering a file a hotspot.
const DefaultHotspotScoreThreshold = 0.5

// CalculateSummary computes summary statistics from file hotspots.
// Files must be sorted by HotspotScore descending before calling.
func (h *HotspotAnalysis) CalculateSummary() {
	if len(h.Files) == 0 {
		return
	}

	h.Summary.TotalFiles = len(h.Files)
	h.Summary.MaxHotspotScore = h.Files[0].HotspotScore

	var sum float64
	scores := make([]float64, len(h.Files))
	for i, f := range h.Files {
		sum += f.HotspotScore
		scores[i] = f.HotspotScore
		if f.HotspotScore >= DefaultHotspotScoreThreshold {
			h.Summary.HotspotCount++
		}
	}

	h.Summary.AvgHotspotScore = sum / float64(len(h.Files))

	// Sort ascending for percentile calculation
	sort.Float64s(scores)
	h.Summary.P50HotspotScore = percentileFloat64(scores, 50)
	h.Summary.P90HotspotScore = percentileFloat64(scores, 90)
}
