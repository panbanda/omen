package featureflags

import (
	"time"
)

// Reference represents a single occurrence of a feature flag in code.
type Reference struct {
	FlagKey      string   `json:"flag_key"`
	Provider     string   `json:"provider"`
	File         string   `json:"file"`
	Line         uint32   `json:"line"`
	Column       uint32   `json:"column"`
	NestingDepth int      `json:"nesting_depth"`
	SiblingFlags []string `json:"sibling_flags,omitempty"`
}

// Complexity captures complexity metrics for a single flag.
type Complexity struct {
	FileSpread      int      `json:"file_spread"`
	MaxNestingDepth int      `json:"max_nesting_depth"`
	DecisionPoints  int      `json:"decision_points"`
	CoupledFlags    []string `json:"coupled_flags,omitempty"`
	CyclomaticDelta int      `json:"cyclomatic_delta"`
}

// Staleness captures git-derived staleness metrics.
type Staleness struct {
	IntroducedAt      *time.Time `json:"introduced_at,omitempty"`
	LastModifiedAt    *time.Time `json:"last_modified_at,omitempty"`
	DaysSinceIntro    int        `json:"days_since_intro,omitempty"`
	DaysSinceModified int        `json:"days_since_modified,omitempty"`
	TotalCommits      int        `json:"total_commits,omitempty"`
	Authors           []string   `json:"authors,omitempty"`
	Score             float64    `json:"score"`
}

// Priority represents the cleanup priority for a flag.
type Priority struct {
	Score float64 `json:"score"`
	Level string  `json:"level"` // LOW, MEDIUM, HIGH, CRITICAL
}

// Priority level constants.
const (
	PriorityLow      = "LOW"
	PriorityMedium   = "MEDIUM"
	PriorityHigh     = "HIGH"
	PriorityCritical = "CRITICAL"
)

// FlagAnalysis represents the complete analysis of a single feature flag.
type FlagAnalysis struct {
	FlagKey    string      `json:"flag_key"`
	Provider   string      `json:"provider"`
	References []Reference `json:"references"`
	Complexity Complexity  `json:"complexity"`
	Staleness  *Staleness  `json:"staleness,omitempty"`
	Priority   Priority    `json:"priority"`
}

// Summary provides aggregate statistics across all flags.
type Summary struct {
	TotalFlags      int            `json:"total_flags"`
	TotalReferences int            `json:"total_references"`
	ByPriority      map[string]int `json:"by_priority"`
	ByProvider      map[string]int `json:"by_provider"`
	TopCoupled      []string       `json:"top_coupled_flags,omitempty"`
	AvgFileSpread   float64        `json:"avg_file_spread"`
	MaxFileSpread   int            `json:"max_file_spread"`
}

// Analysis represents the complete analysis result.
type Analysis struct {
	GeneratedAt time.Time      `json:"generated_at"`
	Flags       []FlagAnalysis `json:"flags"`
	Summary     Summary        `json:"summary"`
}

// NewSummary creates an initialized summary.
func NewSummary() Summary {
	return Summary{
		ByPriority: map[string]int{
			PriorityLow:      0,
			PriorityMedium:   0,
			PriorityHigh:     0,
			PriorityCritical: 0,
		},
		ByProvider: make(map[string]int),
		TopCoupled: make([]string, 0),
	}
}

// CalculateStalenessScore computes the staleness score for a flag.
// Based on research thresholds:
// - 2 points per month overdue past expected TTL
// - 2 points if no modifications in 60+ days
func (s *Staleness) CalculateStalenessScore(expectedTTLDays int) {
	score := 0.0

	if s.DaysSinceIntro > expectedTTLDays {
		overdueDays := s.DaysSinceIntro - expectedTTLDays
		overdueMonths := float64(overdueDays) / 30.0
		score += 2.0 * overdueMonths
	}

	if s.DaysSinceModified > 60 {
		score += 2.0
	}

	s.Score = score
}

// CalculatePriority computes the cleanup priority based on staleness and complexity.
// Based on research-backed formula combining risk and effort.
func CalculatePriority(staleness *Staleness, complexity Complexity) Priority {
	riskScore := 0.0
	if staleness != nil {
		riskScore = staleness.Score
	}

	// Add complexity factors
	if complexity.FileSpread > 10 {
		riskScore += 2.0
	}
	if complexity.MaxNestingDepth > 2 {
		riskScore += 3.0
	}
	if len(complexity.CoupledFlags) > 2 {
		riskScore += 2.0
	}

	// Estimate removal effort (higher spread = more effort)
	effort := float64(complexity.FileSpread) * 0.5
	if effort < 1.0 {
		effort = 1.0
	}
	if complexity.MaxNestingDepth > 1 {
		effort *= 1.5
	}

	priority := riskScore / effort

	var level string
	switch {
	case priority > 20:
		level = PriorityCritical
	case priority > 10:
		level = PriorityHigh
	case priority > 5:
		level = PriorityMedium
	default:
		level = PriorityLow
	}

	return Priority{
		Score: priority,
		Level: level,
	}
}

// Thresholds defines configurable thresholds for flag analysis.
type Thresholds struct {
	ExpectedTTLRelease    int `json:"expected_ttl_release"`    // days, default 14
	ExpectedTTLExperiment int `json:"expected_ttl_experiment"` // days, default 90
	FileSpreadWarning     int `json:"file_spread_warning"`     // default 4
	FileSpreadCritical    int `json:"file_spread_critical"`    // default 10
	NestingWarning        int `json:"nesting_warning"`         // default 2
	NestingCritical       int `json:"nesting_critical"`        // default 3
}

// DefaultThresholds returns sensible defaults based on research.
func DefaultThresholds() Thresholds {
	return Thresholds{
		ExpectedTTLRelease:    14,
		ExpectedTTLExperiment: 90,
		FileSpreadWarning:     4,
		FileSpreadCritical:    10,
		NestingWarning:        2,
		NestingCritical:       3,
	}
}

// IsHighRisk returns true if the flag has concerning metrics.
func (f *FlagAnalysis) IsHighRisk(t Thresholds) bool {
	return f.Complexity.FileSpread >= t.FileSpreadCritical ||
		f.Complexity.MaxNestingDepth >= t.NestingCritical ||
		f.Priority.Level == PriorityCritical ||
		f.Priority.Level == PriorityHigh
}

// FilesAffected returns the unique list of files containing this flag.
func (f *FlagAnalysis) FilesAffected() []string {
	seen := make(map[string]bool)
	files := make([]string, 0)
	for _, ref := range f.References {
		if !seen[ref.File] {
			seen[ref.File] = true
			files = append(files, ref.File)
		}
	}
	return files
}
