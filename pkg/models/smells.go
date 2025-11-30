package models

import "time"

// SmellType represents the type of architectural smell.
type SmellType string

const (
	SmellCyclicDependency   SmellType = "cyclic_dependency"
	SmellHubLikeDependency  SmellType = "hub_like_dependency"
	SmellUnstableDependency SmellType = "unstable_dependency"
	SmellGodComponent       SmellType = "god_component"
)

// SmellSeverity represents the severity level of an architectural smell.
type SmellSeverity string

const (
	SmellSeverityCritical SmellSeverity = "critical"
	SmellSeverityHigh     SmellSeverity = "high"
	SmellSeverityMedium   SmellSeverity = "medium"
)

// ArchitecturalSmell represents a detected architectural smell.
type ArchitecturalSmell struct {
	Type        SmellType     `json:"type"`
	Severity    SmellSeverity `json:"severity"`
	Components  []string      `json:"components"`
	Description string        `json:"description"`
	Suggestion  string        `json:"suggestion"`
	Metrics     SmellMetrics  `json:"metrics,omitempty"`
}

// SmellMetrics provides quantitative data about the smell.
type SmellMetrics struct {
	FanIn       int     `json:"fan_in,omitempty"`
	FanOut      int     `json:"fan_out,omitempty"`
	Instability float64 `json:"instability,omitempty"`
	CycleLength int     `json:"cycle_length,omitempty"`
}

// ComponentMetrics provides instability metrics for a component.
type ComponentMetrics struct {
	ID          string  `json:"id"`
	Name        string  `json:"name"`
	FanIn       int     `json:"fan_in"`      // Afferent coupling (incoming dependencies)
	FanOut      int     `json:"fan_out"`     // Efferent coupling (outgoing dependencies)
	Instability float64 `json:"instability"` // Ce / (Ca + Ce), 0 = stable, 1 = unstable
	IsHub       bool    `json:"is_hub"`      // Fan-in + Fan-out > threshold
	IsGod       bool    `json:"is_god"`      // High fan-in AND high fan-out
}

// Instability calculates Martin's Instability metric.
// I = Ce / (Ca + Ce)
// Where Ce = efferent coupling (outgoing), Ca = afferent coupling (incoming)
// Returns 0 for stable packages (all incoming), 1 for unstable (all outgoing)
func (c *ComponentMetrics) CalculateInstability() float64 {
	total := c.FanIn + c.FanOut
	if total == 0 {
		return 0
	}
	c.Instability = float64(c.FanOut) / float64(total)
	return c.Instability
}

// SmellThresholds configures detection thresholds for architectural smells.
type SmellThresholds struct {
	HubThreshold          int     `json:"hub_threshold"`          // Fan-in + Fan-out threshold for hub detection
	GodFanInThreshold     int     `json:"god_fan_in_threshold"`   // Minimum fan-in for god component
	GodFanOutThreshold    int     `json:"god_fan_out_threshold"`  // Minimum fan-out for god component
	InstabilityDifference float64 `json:"instability_difference"` // Max I difference for unstable dependency
	StableThreshold       float64 `json:"stable_threshold"`       // I < this is considered stable
	UnstableThreshold     float64 `json:"unstable_threshold"`     // I > this is considered unstable
}

// DefaultSmellThresholds returns sensible default thresholds.
func DefaultSmellThresholds() SmellThresholds {
	return SmellThresholds{
		HubThreshold:          20,
		GodFanInThreshold:     10,
		GodFanOutThreshold:    10,
		InstabilityDifference: 0.4,
		StableThreshold:       0.3,
		UnstableThreshold:     0.7,
	}
}

// SmellAnalysis represents the full architectural smell analysis result.
type SmellAnalysis struct {
	GeneratedAt time.Time            `json:"generated_at"`
	Smells      []ArchitecturalSmell `json:"smells"`
	Components  []ComponentMetrics   `json:"components"`
	Summary     SmellSummary         `json:"summary"`
	Thresholds  SmellThresholds      `json:"thresholds"`
}

// SmellSummary provides aggregate statistics.
type SmellSummary struct {
	TotalSmells        int     `json:"total_smells"`
	CyclicCount        int     `json:"cyclic_count"`
	HubCount           int     `json:"hub_count"`
	UnstableCount      int     `json:"unstable_count"`
	GodCount           int     `json:"god_count"`
	CriticalCount      int     `json:"critical_count"`
	HighCount          int     `json:"high_count"`
	MediumCount        int     `json:"medium_count"`
	TotalComponents    int     `json:"total_components"`
	AverageInstability float64 `json:"average_instability"`
}

// NewSmellAnalysis creates an initialized smell analysis.
func NewSmellAnalysis() *SmellAnalysis {
	return &SmellAnalysis{
		GeneratedAt: time.Now().UTC(),
		Smells:      make([]ArchitecturalSmell, 0),
		Components:  make([]ComponentMetrics, 0),
		Thresholds:  DefaultSmellThresholds(),
	}
}

// AddSmell adds a smell and updates the summary.
func (a *SmellAnalysis) AddSmell(smell ArchitecturalSmell) {
	a.Smells = append(a.Smells, smell)
	a.Summary.TotalSmells++

	switch smell.Type {
	case SmellCyclicDependency:
		a.Summary.CyclicCount++
	case SmellHubLikeDependency:
		a.Summary.HubCount++
	case SmellUnstableDependency:
		a.Summary.UnstableCount++
	case SmellGodComponent:
		a.Summary.GodCount++
	}

	switch smell.Severity {
	case SmellSeverityCritical:
		a.Summary.CriticalCount++
	case SmellSeverityHigh:
		a.Summary.HighCount++
	case SmellSeverityMedium:
		a.Summary.MediumCount++
	}
}

// CalculateSummary computes summary statistics from components.
func (a *SmellAnalysis) CalculateSummary() {
	if len(a.Components) == 0 {
		return
	}

	a.Summary.TotalComponents = len(a.Components)

	var sum float64
	for _, c := range a.Components {
		sum += c.Instability
	}
	a.Summary.AverageInstability = sum / float64(len(a.Components))
}
