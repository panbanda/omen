package models

import (
	"time"
)

// FlagReference represents a single occurrence of a feature flag in code.
type FlagReference struct {
	FlagKey      string   `json:"flag_key"`
	Provider     string   `json:"provider"`
	File         string   `json:"file"`
	Line         uint32   `json:"line"`
	Column       uint32   `json:"column"`
	NestingDepth int      `json:"nesting_depth"`
	SiblingFlags []string `json:"sibling_flags,omitempty"`
}

// FlagComplexity captures complexity metrics for a single flag.
type FlagComplexity struct {
	FileSpread      int      `json:"file_spread"`
	MaxNestingDepth int      `json:"max_nesting_depth"`
	DecisionPoints  int      `json:"decision_points"`
	CoupledFlags    []string `json:"coupled_flags,omitempty"`
	CyclomaticDelta int      `json:"cyclomatic_delta"`
}

// FlagStaleness captures git-derived staleness metrics.
type FlagStaleness struct {
	IntroducedAt      *time.Time `json:"introduced_at,omitempty"`
	LastModifiedAt    *time.Time `json:"last_modified_at,omitempty"`
	DaysSinceIntro    int        `json:"days_since_intro,omitempty"`
	DaysSinceModified int        `json:"days_since_modified,omitempty"`
	TotalCommits      int        `json:"total_commits,omitempty"`
	Authors           []string   `json:"authors,omitempty"`
	Score             float64    `json:"score"`
}

// FlagPriority represents the cleanup priority for a flag.
type FlagPriority struct {
	Score float64 `json:"score"`
	Level string  `json:"level"` // LOW, MEDIUM, HIGH, CRITICAL
}

// PriorityLevel constants.
const (
	PriorityLow      = "LOW"
	PriorityMedium   = "MEDIUM"
	PriorityHigh     = "HIGH"
	PriorityCritical = "CRITICAL"
)

// FlagAnalysis represents the complete analysis of a single feature flag.
type FlagAnalysis struct {
	FlagKey    string          `json:"flag_key"`
	Provider   string          `json:"provider"`
	References []FlagReference `json:"references"`
	Complexity FlagComplexity  `json:"complexity"`
	Staleness  *FlagStaleness  `json:"staleness,omitempty"`
	Priority   FlagPriority    `json:"priority"`
}

// FlagAnalysisSummary provides aggregate statistics across all flags.
type FlagAnalysisSummary struct {
	TotalFlags      int            `json:"total_flags"`
	TotalReferences int            `json:"total_references"`
	ByPriority      map[string]int `json:"by_priority"`
	ByProvider      map[string]int `json:"by_provider"`
	TopCoupled      []string       `json:"top_coupled_flags,omitempty"`
	AvgFileSpread   float64        `json:"avg_file_spread"`
	MaxFileSpread   int            `json:"max_file_spread"`
}

// FeatureFlagAnalysis represents the complete analysis result.
type FeatureFlagAnalysis struct {
	GeneratedAt time.Time           `json:"generated_at"`
	Flags       []FlagAnalysis      `json:"flags"`
	Summary     FlagAnalysisSummary `json:"summary"`
}

// NewFlagAnalysisSummary creates an initialized summary.
func NewFlagAnalysisSummary() FlagAnalysisSummary {
	return FlagAnalysisSummary{
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
func (s *FlagStaleness) CalculateStalenessScore(expectedTTLDays int) {
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
func CalculatePriority(staleness *FlagStaleness, complexity FlagComplexity) FlagPriority {
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

	return FlagPriority{
		Score: priority,
		Level: level,
	}
}

// FlagThresholds defines configurable thresholds for flag analysis.
type FlagThresholds struct {
	ExpectedTTLRelease    int `json:"expected_ttl_release"`    // days, default 14
	ExpectedTTLExperiment int `json:"expected_ttl_experiment"` // days, default 90
	FileSpreadWarning     int `json:"file_spread_warning"`     // default 4
	FileSpreadCritical    int `json:"file_spread_critical"`    // default 10
	NestingWarning        int `json:"nesting_warning"`         // default 2
	NestingCritical       int `json:"nesting_critical"`        // default 3
}

// DefaultFlagThresholds returns sensible defaults based on research.
func DefaultFlagThresholds() FlagThresholds {
	return FlagThresholds{
		ExpectedTTLRelease:    14,
		ExpectedTTLExperiment: 90,
		FileSpreadWarning:     4,
		FileSpreadCritical:    10,
		NestingWarning:        2,
		NestingCritical:       3,
	}
}

// IsHighRisk returns true if the flag has concerning metrics.
func (f *FlagAnalysis) IsHighRisk(t FlagThresholds) bool {
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
