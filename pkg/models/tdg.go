package models

// TDGSeverity represents the Technical Debt Gradient severity level.
type TDGSeverity string

const (
	TDGNormal   TDGSeverity = "normal"   // < 1.5
	TDGWarning  TDGSeverity = "warning"  // 1.5 - 2.5
	TDGCritical TDGSeverity = "critical" // > 2.5
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
	Value      float64       `json:"value"` // 0-5 scale (higher is more debt)
	Severity   TDGSeverity   `json:"severity"`
	Components TDGComponents `json:"components"`
	Confidence float64       `json:"confidence"` // 0-1, reduced for missing data
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
	MaxScore   float64        `json:"max_score"` // Worst (highest debt) score
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
		Churn:       0.35,
		Coupling:    0.15,
		Duplication: 0.10,
		DomainRisk:  0.10,
	}
}

// CalculateTDGSeverity determines severity from score (0-5 scale, higher is more debt).
func CalculateTDGSeverity(score float64) TDGSeverity {
	switch {
	case score > 2.5:
		return TDGCritical
	case score >= 1.5:
		return TDGWarning
	default:
		return TDGNormal
	}
}

// CalculateTDG computes the TDG score from components.
// Returns a score from 0-5 where higher is more debt.
// Components are normalized to 0-5 scale individually.
func CalculateTDG(c TDGComponents, w TDGWeights) float64 {
	score := c.Complexity*w.Complexity +
		c.Churn*w.Churn +
		c.Coupling*w.Coupling +
		c.Duplication*w.Duplication +
		c.DomainRisk*w.DomainRisk

	if score < 0 {
		score = 0
	}
	if score > 5.0 {
		score = 5.0
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

// Color returns an ANSI color code for the severity.
func (s TDGSeverity) Color() string {
	switch s {
	case TDGNormal:
		return "\033[32m" // Green
	case TDGWarning:
		return "\033[33m" // Yellow
	case TDGCritical:
		return "\033[31m" // Red
	default:
		return "\033[0m" // Reset
	}
}
