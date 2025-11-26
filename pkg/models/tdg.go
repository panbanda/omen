package models

// TDGSeverity represents the Technical Debt Gradient severity level.
type TDGSeverity string

const (
	TDGExcellent TDGSeverity = "excellent" // 90-100
	TDGGood      TDGSeverity = "good"      // 70-89
	TDGModerate  TDGSeverity = "moderate"  // 50-69
	TDGHighRisk  TDGSeverity = "high_risk" // 0-49
)

// TDGComponents represents the individual factors in TDG calculation.
type TDGComponents struct {
	Complexity  float64 `json:"complexity"`
	Churn       float64 `json:"churn"`
	Coupling    float64 `json:"coupling"`
	Duplication float64 `json:"duplication"`
	DomainRisk  float64 `json:"domain_risk"`
}

// TDGScore represents the Technical Debt Gradient for a file.
type TDGScore struct {
	FilePath   string        `json:"file_path"`
	Value      float64       `json:"value"` // 0-100 scale (higher is better)
	Severity   TDGSeverity   `json:"severity"`
	Components TDGComponents `json:"components"`
}

// TDGAnalysis represents the full TDG analysis result.
type TDGAnalysis struct {
	Files   []TDGScore `json:"files"`
	Summary TDGSummary `json:"summary"`
}

// TDGSummary provides aggregate statistics.
type TDGSummary struct {
	TotalFiles int            `json:"total_files"`
	AvgScore   float64        `json:"avg_score"`
	MaxScore   float64        `json:"min_score"` // Worst (lowest) score in the codebase
	P50Score   float64        `json:"p50_score"`
	P95Score   float64        `json:"p95_score"`
	BySeverity map[string]int `json:"by_severity"`
	Hotspots   []TDGScore     `json:"hotspots"` // Top N worst scoring files
}

// TDGWeights defines the weights for TDG calculation.
type TDGWeights struct {
	Complexity  float64 `json:"complexity"`
	Churn       float64 `json:"churn"`
	Coupling    float64 `json:"coupling"`
	Duplication float64 `json:"duplication"`
	DomainRisk  float64 `json:"domain_risk"`
}

// DefaultTDGWeights returns the standard weights.
func DefaultTDGWeights() TDGWeights {
	return TDGWeights{
		Complexity:  0.30,
		Churn:       0.25,
		Coupling:    0.20,
		Duplication: 0.15,
		DomainRisk:  0.10,
	}
}

// CalculateTDGSeverity determines severity from score (0-100 scale, higher is better).
func CalculateTDGSeverity(score float64) TDGSeverity {
	switch {
	case score >= 90:
		return TDGExcellent
	case score >= 70:
		return TDGGood
	case score >= 50:
		return TDGModerate
	default:
		return TDGHighRisk
	}
}

// CalculateTDG computes the TDG score from components.
// Returns a score from 0-100 where higher is better (less debt).
// Components are penalties (0-1 normalized), so we subtract from 100.
func CalculateTDG(c TDGComponents, w TDGWeights) float64 {
	penalty := c.Complexity*w.Complexity +
		c.Churn*w.Churn +
		c.Coupling*w.Coupling +
		c.Duplication*w.Duplication +
		c.DomainRisk*w.DomainRisk

	// Convert penalty (0-1) to score (0-100) where higher is better
	score := (1.0 - penalty) * 100.0
	if score < 0 {
		score = 0
	}
	if score > 100 {
		score = 100
	}
	return score
}

// NewTDGSummary creates an initialized summary.
func NewTDGSummary() TDGSummary {
	return TDGSummary{
		BySeverity: make(map[string]int),
		Hotspots:   make([]TDGScore, 0),
	}
}

// SeverityColor returns an ANSI color code for the severity.
func (s TDGSeverity) Color() string {
	switch s {
	case TDGExcellent:
		return "\033[32m" // Green
	case TDGGood:
		return "\033[33m" // Yellow
	case TDGModerate:
		return "\033[38;5;208m" // Orange
	case TDGHighRisk:
		return "\033[31m" // Red
	default:
		return "\033[0m" // Reset
	}
}
