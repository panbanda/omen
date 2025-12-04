package hotspot

import (
	"math"
	"sort"
	"time"

	"github.com/panbanda/omen/pkg/stats"
)

// Empirical CDF percentiles for hotspot churn (commits in 30 days).
// Derived from analysis of OSS project distributions. Most files in a healthy
// codebase have 0-2 commits per month; files with 5+ commits are notably active.
//
// References:
//   - pmat defect_probability.rs: empirical CDFs from 10K+ OSS projects
//   - Tornhill, "Your Code as a Crime Scene" (2015): change frequency correlates with defects
//   - Nagappan et al., "Use of Relative Code Churn Measures to Predict System Defect Density" (ICSE 2005)
var churnCDF = [][2]float64{
	{0, 0.0},   // No changes
	{1, 0.30},  // Single commit - common for stable files
	{2, 0.50},  // Two commits - median activity
	{3, 0.60},  // Three commits - above average
	{5, 0.75},  // Five commits - high activity
	{7, 0.85},  // Seven commits - very active
	{10, 0.92}, // Ten commits - hotspot territory
	{15, 0.96}, // Fifteen commits - extreme churn
	{20, 0.98}, // Twenty commits - critical
	{50, 1.0},  // Fifty+ commits - outlier
}

// Empirical CDF percentiles for hotspot complexity (average cognitive per function).
// Based on industry benchmarks: SonarQube considers cognitive complexity > 15 as high.
//
// References:
//   - pmat defect_probability.rs: empirical CDFs for cyclomatic complexity
//   - SonarSource cognitive complexity whitepaper (2017)
//   - McCabe, "A Complexity Measure" (IEEE TSE 1976): cyclomatic complexity thresholds
var complexityCDF = [][2]float64{
	{0, 0.0},   // No complexity (unlikely)
	{1, 0.10},  // Trivial functions
	{2, 0.20},  // Simple functions
	{3, 0.30},  // Low complexity
	{5, 0.50},  // Moderate - median
	{7, 0.70},  // Above average
	{10, 0.80}, // Complex
	{15, 0.90}, // High complexity (SonarQube threshold)
	{20, 0.95}, // Very high
	{30, 0.98}, // Extreme
	{50, 1.0},  // Outlier
}

// NormalizeChurnCDF normalizes commit count using empirical CDF.
// Returns a value between 0 and 1 representing the percentile.
func NormalizeChurnCDF(commits int) float64 {
	return interpolateCDF(churnCDF, float64(commits))
}

// NormalizeComplexityCDF normalizes average cognitive complexity using empirical CDF.
// Returns a value between 0 and 1 representing the percentile.
func NormalizeComplexityCDF(avgCognitive float64) float64 {
	return interpolateCDF(complexityCDF, avgCognitive)
}

// CalculateScore computes the hotspot score using geometric mean.
// This preserves the "intersection" semantics: both churn AND complexity
// must be elevated for a high score.
func CalculateScore(churnNorm, complexityNorm float64) float64 {
	if churnNorm <= 0 || complexityNorm <= 0 {
		return 0
	}
	return math.Sqrt(churnNorm * complexityNorm)
}

// interpolateCDF performs linear interpolation on empirical CDF percentiles.
func interpolateCDF(cdf [][2]float64, value float64) float64 {
	if value <= cdf[0][0] {
		return cdf[0][1]
	}
	last := len(cdf) - 1
	if value >= cdf[last][0] {
		return cdf[last][1]
	}

	for i := 0; i < last; i++ {
		v1, p1 := cdf[i][0], cdf[i][1]
		v2, p2 := cdf[i+1][0], cdf[i+1][1]

		if value >= v1 && value <= v2 {
			t := (value - v1) / (v2 - v1)
			return p1 + t*(p2-p1)
		}
	}
	return 0
}

// FileHotspot represents hotspot metrics for a single file.
type FileHotspot struct {
	Path            string  `json:"path"`
	HotspotScore    float64 `json:"hotspot_score"`    // 0-1, geometric mean of normalized churn and complexity
	ChurnScore      float64 `json:"churn_score"`      // 0-1, CDF-normalized
	ComplexityScore float64 `json:"complexity_score"` // 0-1, CDF-normalized
	Commits         int     `json:"commits"`
	AvgCognitive    float64 `json:"avg_cognitive"`
	AvgCyclomatic   float64 `json:"avg_cyclomatic"`
	TotalFunctions  int     `json:"total_functions"`
}

// Summary provides aggregate statistics for hotspot analysis.
type Summary struct {
	TotalFiles      int     `json:"total_files"`
	HotspotCount    int     `json:"hotspot_count"` // Files above threshold
	MaxHotspotScore float64 `json:"max_hotspot_score"`
	AvgHotspotScore float64 `json:"avg_hotspot_score"`
	P50HotspotScore float64 `json:"p50_hotspot_score"`
	P90HotspotScore float64 `json:"p90_hotspot_score"`
}

// Analysis represents the full hotspot analysis result.
type Analysis struct {
	GeneratedAt time.Time     `json:"generated_at"`
	PeriodDays  int           `json:"period_days"`
	Files       []FileHotspot `json:"files"`
	Summary     Summary       `json:"summary"`
}

// Hotspot severity thresholds based on geometric mean of CDF-normalized scores.
const (
	// CriticalThreshold indicates a critical hotspot requiring immediate attention.
	// Files scoring >= 0.6 have both high churn AND high complexity.
	CriticalThreshold = 0.6

	// HighThreshold indicates a significant hotspot that should be reviewed.
	HighThreshold = 0.4

	// ModerateThreshold indicates a file worth monitoring.
	ModerateThreshold = 0.25

	// DefaultScoreThreshold is the default threshold for counting hotspots in summary.
	// Uses the "High" threshold as the default.
	DefaultScoreThreshold = HighThreshold
)

// Severity represents the severity level of a hotspot.
type Severity string

const (
	SeverityCritical Severity = "critical"
	SeverityHigh     Severity = "high"
	SeverityModerate Severity = "moderate"
	SeverityLow      Severity = "low"
)

// GetSeverity returns the severity level based on the hotspot score.
func (f *FileHotspot) GetSeverity() Severity {
	switch {
	case f.HotspotScore >= CriticalThreshold:
		return SeverityCritical
	case f.HotspotScore >= HighThreshold:
		return SeverityHigh
	case f.HotspotScore >= ModerateThreshold:
		return SeverityModerate
	default:
		return SeverityLow
	}
}

// CalculateSummary computes summary statistics from file hotspots.
// Files must be sorted by HotspotScore descending before calling.
func (h *Analysis) CalculateSummary() {
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
		if f.HotspotScore >= DefaultScoreThreshold {
			h.Summary.HotspotCount++
		}
	}

	h.Summary.AvgHotspotScore = sum / float64(len(h.Files))

	// Sort ascending for percentile calculation
	sort.Float64s(scores)
	h.Summary.P50HotspotScore = stats.Percentile(scores, 50)
	h.Summary.P90HotspotScore = stats.Percentile(scores, 90)
}
