package complexity

// Metrics represents code complexity measurements for a function or file.
type Metrics struct {
	Cyclomatic uint32 `json:"cyclomatic"`
	Cognitive  uint32 `json:"cognitive"`
	MaxNesting int    `json:"max_nesting"`
	Lines      int    `json:"lines"`
}

// FunctionResult represents complexity metrics for a single function.
type FunctionResult struct {
	Name       string   `json:"name"`
	File       string   `json:"file"`
	StartLine  uint32   `json:"start_line"`
	EndLine    uint32   `json:"end_line"`
	Metrics    Metrics  `json:"metrics"`
	Violations []string `json:"violations,omitempty"`
}

// FileResult represents aggregated complexity for a file.
type FileResult struct {
	Path            string           `json:"path"`
	Language        string           `json:"language"`
	Functions       []FunctionResult `json:"functions"`
	TotalCyclomatic uint32           `json:"total_cyclomatic"`
	TotalCognitive  uint32           `json:"total_cognitive"`
	AvgCyclomatic   float64          `json:"avg_cyclomatic"`
	AvgCognitive    float64          `json:"avg_cognitive"`
	MaxCyclomatic   uint32           `json:"max_cyclomatic"`
	MaxCognitive    uint32           `json:"max_cognitive"`
	ViolationCount  int              `json:"violation_count"`
}

// Analysis represents the full analysis result.
type Analysis struct {
	Files   []FileResult `json:"files"`
	Summary Summary      `json:"summary"`
}

// Summary provides aggregate statistics.
type Summary struct {
	TotalFiles     int     `json:"total_files"`
	TotalFunctions int     `json:"total_functions"`
	AvgCyclomatic  float64 `json:"avg_cyclomatic"`
	AvgCognitive   float64 `json:"avg_cognitive"`
	MaxCyclomatic  uint32  `json:"max_cyclomatic"`
	MaxCognitive   uint32  `json:"max_cognitive"`
	P50Cyclomatic  uint32  `json:"p50_cyclomatic"`
	P90Cyclomatic  uint32  `json:"p90_cyclomatic"`
	P95Cyclomatic  uint32  `json:"p95_cyclomatic"`
	P50Cognitive   uint32  `json:"p50_cognitive"`
	P90Cognitive   uint32  `json:"p90_cognitive"`
	P95Cognitive   uint32  `json:"p95_cognitive"`
	ViolationCount int     `json:"violation_count"`
}

// Thresholds defines the limits for complexity violations.
type Thresholds struct {
	MaxCyclomatic uint32 `json:"max_cyclomatic"`
	MaxCognitive  uint32 `json:"max_cognitive"`
	MaxNesting    int    `json:"max_nesting"`
}

// DefaultThresholds returns sensible defaults.
func DefaultThresholds() Thresholds {
	return Thresholds{
		MaxCyclomatic: 10,
		MaxCognitive:  15,
		MaxNesting:    4,
	}
}

// ExtendedThresholds provides warn and error levels (pmat compatible).
type ExtendedThresholds struct {
	CyclomaticWarn  uint32 `json:"cyclomatic_warn"`
	CyclomaticError uint32 `json:"cyclomatic_error"`
	CognitiveWarn   uint32 `json:"cognitive_warn"`
	CognitiveError  uint32 `json:"cognitive_error"`
	NestingMax      uint8  `json:"nesting_max"`
	MethodLength    uint16 `json:"method_length"`
}

// DefaultExtendedThresholds returns pmat-compatible default thresholds.
func DefaultExtendedThresholds() ExtendedThresholds {
	return ExtendedThresholds{
		CyclomaticWarn:  10,
		CyclomaticError: 20,
		CognitiveWarn:   15,
		CognitiveError:  30,
		NestingMax:      5,
		MethodLength:    50,
	}
}

// ViolationSeverity indicates the severity of a complexity violation.
type ViolationSeverity string

const (
	SeverityWarning ViolationSeverity = "warning"
	SeverityError   ViolationSeverity = "error"
)

// Violation represents a complexity threshold violation.
type Violation struct {
	Severity  ViolationSeverity `json:"severity"`
	Rule      string            `json:"rule"`
	Message   string            `json:"message"`
	Value     uint32            `json:"value"`
	Threshold uint32            `json:"threshold"`
	File      string            `json:"file"`
	Line      uint32            `json:"line"`
	Function  string            `json:"function,omitempty"`
}

// Hotspot identifies a high-complexity location in the codebase.
type Hotspot struct {
	File           string `json:"file"`
	Function       string `json:"function,omitempty"`
	Line           uint32 `json:"line"`
	Complexity     uint32 `json:"complexity"`
	ComplexityType string `json:"complexity_type"`
}

// Report is the full analysis report with violations and hotspots.
type Report struct {
	Summary            ExtendedSummary `json:"summary"`
	Violations         []Violation     `json:"violations"`
	Hotspots           []Hotspot       `json:"hotspots"`
	Files              []FileResult    `json:"files"`
	TechnicalDebtHours float32         `json:"technical_debt_hours"`
}

// ExtendedSummary provides enhanced statistics (pmat compatible).
type ExtendedSummary struct {
	TotalFiles         int     `json:"total_files"`
	TotalFunctions     int     `json:"total_functions"`
	MedianCyclomatic   float32 `json:"median_cyclomatic"`
	MedianCognitive    float32 `json:"median_cognitive"`
	MaxCyclomatic      uint32  `json:"max_cyclomatic"`
	MaxCognitive       uint32  `json:"max_cognitive"`
	P90Cyclomatic      uint32  `json:"p90_cyclomatic"`
	P90Cognitive       uint32  `json:"p90_cognitive"`
	TechnicalDebtHours float32 `json:"technical_debt_hours"`
}

// IsSimple returns true if complexity is within acceptable limits.
func (m *Metrics) IsSimple(t Thresholds) bool {
	return m.Cyclomatic <= t.MaxCyclomatic &&
		m.Cognitive <= t.MaxCognitive &&
		m.MaxNesting <= t.MaxNesting
}

// NeedsRefactoring returns true if any metric significantly exceeds thresholds.
func (m *Metrics) NeedsRefactoring(t Thresholds) bool {
	return m.Cyclomatic > t.MaxCyclomatic*2 ||
		m.Cognitive > t.MaxCognitive*2 ||
		m.MaxNesting > t.MaxNesting*2
}

// IsSimpleDefault checks if complexity is low using fixed thresholds (pmat compatible).
// Returns true if cyclomatic <= 5 and cognitive <= 7.
func (m *Metrics) IsSimpleDefault() bool {
	return m.Cyclomatic <= 5 && m.Cognitive <= 7
}

// NeedsRefactoringDefault checks if complexity exceeds fixed thresholds (pmat compatible).
// Returns true if cyclomatic > 10 or cognitive > 15.
func (m *Metrics) NeedsRefactoringDefault() bool {
	return m.Cyclomatic > 10 || m.Cognitive > 15
}

// ComplexityScore calculates a composite complexity score for ranking.
// Combines cyclomatic, cognitive, nesting, and lines with weighted factors.
func (m *Metrics) ComplexityScore() float64 {
	return float64(m.Cyclomatic)*1.0 +
		float64(m.Cognitive)*1.2 +
		float64(m.MaxNesting)*2.0 +
		float64(m.Lines)*0.1
}
