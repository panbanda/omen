package smells

import "time"

// Type represents the type of architectural smell.
type Type string

const (
	TypeCyclicDependency   Type = "cyclic_dependency"
	TypeHubLikeDependency  Type = "hub_like_dependency"
	TypeUnstableDependency Type = "unstable_dependency"
	TypeGodComponent       Type = "god_component"
)

// Severity represents the severity level of an architectural smell.
type Severity string

const (
	SeverityCritical Severity = "critical"
	SeverityHigh     Severity = "high"
	SeverityMedium   Severity = "medium"
)

// Weight returns a numeric weight for sorting (higher = more severe).
func (s Severity) Weight() int {
	switch s {
	case SeverityCritical:
		return 3
	case SeverityHigh:
		return 2
	case SeverityMedium:
		return 1
	default:
		return 0
	}
}

// Smell represents a detected architectural smell.
type Smell struct {
	Type        Type     `json:"type"`
	Severity    Severity `json:"severity"`
	Components  []string `json:"components"`
	Description string   `json:"description"`
	Suggestion  string   `json:"suggestion"`
	Metrics     Metrics  `json:"metrics,omitempty"`
}

// Metrics provides quantitative data about the smell.
type Metrics struct {
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

// CalculateInstability calculates Martin's Instability metric.
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

// Thresholds configures detection thresholds for architectural smells.
type Thresholds struct {
	HubThreshold          int     `json:"hub_threshold"`          // Fan-in + Fan-out threshold for hub detection
	GodFanInThreshold     int     `json:"god_fan_in_threshold"`   // Minimum fan-in for god component
	GodFanOutThreshold    int     `json:"god_fan_out_threshold"`  // Minimum fan-out for god component
	InstabilityDifference float64 `json:"instability_difference"` // Max I difference for unstable dependency
	StableThreshold       float64 `json:"stable_threshold"`       // I < this is considered stable
	UnstableThreshold     float64 `json:"unstable_threshold"`     // I > this is considered unstable
}

// DefaultThresholds returns sensible default thresholds.
func DefaultThresholds() Thresholds {
	return Thresholds{
		HubThreshold:          20,
		GodFanInThreshold:     10,
		GodFanOutThreshold:    10,
		InstabilityDifference: 0.4,
		StableThreshold:       0.3,
		UnstableThreshold:     0.7,
	}
}

// Summary provides aggregate statistics.
type Summary struct {
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

// Analysis represents the full architectural smell analysis result.
type Analysis struct {
	GeneratedAt time.Time          `json:"generated_at"`
	Smells      []Smell            `json:"smells"`
	Components  []ComponentMetrics `json:"components"`
	Summary     Summary            `json:"summary"`
	Thresholds  Thresholds         `json:"thresholds"`
}

// NewAnalysis creates an initialized smell analysis.
func NewAnalysis() *Analysis {
	return &Analysis{
		GeneratedAt: time.Now().UTC(),
		Smells:      make([]Smell, 0),
		Components:  make([]ComponentMetrics, 0),
		Thresholds:  DefaultThresholds(),
	}
}

// AddSmell adds a smell and updates the summary.
func (a *Analysis) AddSmell(smell Smell) {
	a.Smells = append(a.Smells, smell)
	a.Summary.TotalSmells++

	switch smell.Type {
	case TypeCyclicDependency:
		a.Summary.CyclicCount++
	case TypeHubLikeDependency:
		a.Summary.HubCount++
	case TypeUnstableDependency:
		a.Summary.UnstableCount++
	case TypeGodComponent:
		a.Summary.GodCount++
	}

	switch smell.Severity {
	case SeverityCritical:
		a.Summary.CriticalCount++
	case SeverityHigh:
		a.Summary.HighCount++
	case SeverityMedium:
		a.Summary.MediumCount++
	}
}

// CalculateSummary computes summary statistics from components.
func (a *Analysis) CalculateSummary() {
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
