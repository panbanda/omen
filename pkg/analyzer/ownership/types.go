package ownership

import (
	"sort"
	"time"
)

// Contributor represents a contributor to a file.
type Contributor struct {
	Name       string  `json:"name"`
	Email      string  `json:"email"`
	LinesOwned int     `json:"lines_owned"`
	Percentage float64 `json:"percentage"` // 0-100
}

// FileOwnership represents ownership metrics for a single file.
type FileOwnership struct {
	Path             string        `json:"path"`
	PrimaryOwner     string        `json:"primary_owner"`
	OwnershipPercent float64       `json:"ownership_percent"` // 0-100
	Concentration    float64       `json:"concentration"`     // 0-1, higher = more concentrated
	TotalLines       int           `json:"total_lines"`
	Contributors     []Contributor `json:"contributors,omitempty"`
	IsSilo           bool          `json:"is_silo"` // Single contributor
}

// Summary provides aggregate statistics.
type Summary struct {
	TotalFiles       int      `json:"total_files"`
	BusFactor        int      `json:"bus_factor"`
	SiloCount        int      `json:"silo_count"`
	AvgContributors  float64  `json:"avg_contributors"`
	MaxConcentration float64  `json:"max_concentration"`
	TopContributors  []string `json:"top_contributors"`
}

// Analysis represents the full ownership analysis result.
type Analysis struct {
	GeneratedAt time.Time       `json:"generated_at"`
	Files       []FileOwnership `json:"files"`
	Summary     Summary         `json:"summary"`
}

// CalculateSummary computes summary statistics.
func (o *Analysis) CalculateSummary() {
	if len(o.Files) == 0 {
		return
	}

	o.Summary.TotalFiles = len(o.Files)

	// Count silos and calculate average contributors
	contributorCounts := make(map[string]int)
	var totalContributors int
	var maxConcentration float64

	for _, f := range o.Files {
		if f.IsSilo {
			o.Summary.SiloCount++
		}
		totalContributors += len(f.Contributors)
		if f.Concentration > maxConcentration {
			maxConcentration = f.Concentration
		}

		// Track overall contribution
		for _, c := range f.Contributors {
			contributorCounts[c.Name] += c.LinesOwned
		}
	}

	o.Summary.AvgContributors = float64(totalContributors) / float64(len(o.Files))
	o.Summary.MaxConcentration = maxConcentration

	// Calculate bus factor - minimum contributors covering 50% of codebase
	o.Summary.BusFactor = calculateBusFactor(contributorCounts)

	// Get top contributors
	o.Summary.TopContributors = getTopContributors(contributorCounts, 5)
}

// calculateBusFactor returns the minimum number of contributors
// who together own at least 50% of the codebase.
func calculateBusFactor(contributorCounts map[string]int) int {
	if len(contributorCounts) == 0 {
		return 0
	}

	// Calculate total lines
	var total int
	for _, count := range contributorCounts {
		total += count
	}
	if total == 0 {
		return 0
	}

	// Sort contributors by lines (descending)
	type kv struct {
		name  string
		lines int
	}
	var sorted []kv
	for name, lines := range contributorCounts {
		sorted = append(sorted, kv{name, lines})
	}
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].lines > sorted[j].lines
	})

	// Count how many needed to reach 50%
	threshold := total / 2
	var accumulated int
	for i, kv := range sorted {
		accumulated += kv.lines
		if accumulated >= threshold {
			return i + 1
		}
	}

	return len(sorted)
}

// getTopContributors returns the top N contributors by total lines.
func getTopContributors(contributorCounts map[string]int, n int) []string {
	type kv struct {
		name  string
		lines int
	}
	var sorted []kv
	for name, lines := range contributorCounts {
		sorted = append(sorted, kv{name, lines})
	}
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].lines > sorted[j].lines
	})

	var result []string
	for i, kv := range sorted {
		if i >= n {
			break
		}
		result = append(result, kv.name)
	}
	return result
}

// CalculateConcentration computes ownership concentration (0-1).
// Uses simplified Gini-like coefficient: top owner's percentage / 100.
func CalculateConcentration(contributors []Contributor) float64 {
	if len(contributors) == 0 {
		return 0
	}
	if len(contributors) == 1 {
		return 1.0 // Single owner = maximum concentration
	}

	// Find max percentage
	var maxPct float64
	for _, c := range contributors {
		if c.Percentage > maxPct {
			maxPct = c.Percentage
		}
	}
	return maxPct / 100.0
}
