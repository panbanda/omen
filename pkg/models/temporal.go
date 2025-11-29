package models

import "time"

// FileCoupling represents the temporal coupling between two files.
type FileCoupling struct {
	FileA            string  `json:"file_a"`
	FileB            string  `json:"file_b"`
	CochangeCount    int     `json:"cochange_count"`
	CouplingStrength float64 `json:"coupling_strength"` // 0-1
	CommitsA         int     `json:"commits_a"`
	CommitsB         int     `json:"commits_b"`
}

// TemporalCouplingSummary provides aggregate statistics.
type TemporalCouplingSummary struct {
	TotalCouplings      int     `json:"total_couplings"`
	StrongCouplings     int     `json:"strong_couplings"` // Strength >= 0.5
	AvgCouplingStrength float64 `json:"avg_coupling_strength"`
	MaxCouplingStrength float64 `json:"max_coupling_strength"`
	TotalFilesAnalyzed  int     `json:"total_files_analyzed"`
}

// TemporalCouplingAnalysis represents the full temporal coupling analysis result.
type TemporalCouplingAnalysis struct {
	GeneratedAt  time.Time               `json:"generated_at"`
	PeriodDays   int                     `json:"period_days"`
	MinCochanges int                     `json:"min_cochanges"`
	Couplings    []FileCoupling          `json:"couplings"`
	Summary      TemporalCouplingSummary `json:"summary"`
}

// DefaultMinCochanges is the minimum co-change count to consider files coupled.
const DefaultMinCochanges = 3

// StrongCouplingThreshold is the threshold for considering coupling "strong".
const StrongCouplingThreshold = 0.5

// CalculateSummary computes summary statistics from couplings.
// Couplings should be sorted by CouplingStrength descending before calling.
func (t *TemporalCouplingAnalysis) CalculateSummary(totalFiles int) {
	t.Summary.TotalFilesAnalyzed = totalFiles
	t.Summary.TotalCouplings = len(t.Couplings)

	if len(t.Couplings) == 0 {
		return
	}

	t.Summary.MaxCouplingStrength = t.Couplings[0].CouplingStrength

	var sum float64
	for _, c := range t.Couplings {
		sum += c.CouplingStrength
		if c.CouplingStrength >= StrongCouplingThreshold {
			t.Summary.StrongCouplings++
		}
	}

	t.Summary.AvgCouplingStrength = sum / float64(len(t.Couplings))
}

// CalculateCouplingStrength computes the coupling strength between two files.
// Strength = cochanges / max(commitsA, commitsB)
func CalculateCouplingStrength(cochanges, commitsA, commitsB int) float64 {
	maxCommits := commitsA
	if commitsB > maxCommits {
		maxCommits = commitsB
	}
	if maxCommits == 0 {
		return 0
	}
	strength := float64(cochanges) / float64(maxCommits)
	if strength > 1.0 {
		strength = 1.0
	}
	return strength
}
