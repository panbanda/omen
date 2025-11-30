package models

// ComplexityMetrics represents code complexity measurements for a function or file.
type ComplexityMetrics struct {
	Cyclomatic uint32 `json:"cyclomatic"`
	Cognitive  uint32 `json:"cognitive"`
	MaxNesting int    `json:"max_nesting"`
	Lines      int    `json:"lines"`
}

// FunctionComplexity represents complexity metrics for a single function.
type FunctionComplexity struct {
	Name       string            `json:"name"`
	File       string            `json:"file"`
	StartLine  uint32            `json:"start_line"`
	EndLine    uint32            `json:"end_line"`
	Metrics    ComplexityMetrics `json:"metrics"`
	Violations []string          `json:"violations,omitempty"`
}

// FileComplexity represents aggregated complexity for a file.
type FileComplexity struct {
	Path            string               `json:"path"`
	Language        string               `json:"language"`
	Functions       []FunctionComplexity `json:"functions"`
	TotalCyclomatic uint32               `json:"total_cyclomatic"`
	TotalCognitive  uint32               `json:"total_cognitive"`
	AvgCyclomatic   float64              `json:"avg_cyclomatic"`
	AvgCognitive    float64              `json:"avg_cognitive"`
	MaxCyclomatic   uint32               `json:"max_cyclomatic"`
	MaxCognitive    uint32               `json:"max_cognitive"`
	ViolationCount  int                  `json:"violation_count"`
}

// ComplexityAnalysis represents the full analysis result.
type ComplexityAnalysis struct {
	Files   []FileComplexity  `json:"files"`
	Summary ComplexitySummary `json:"summary"`
}

// ComplexitySummary provides aggregate statistics.
type ComplexitySummary struct {
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

// ComplexityThresholds defines the limits for complexity violations.
type ComplexityThresholds struct {
	MaxCyclomatic uint32 `json:"max_cyclomatic"`
	MaxCognitive  uint32 `json:"max_cognitive"`
	MaxNesting    int    `json:"max_nesting"`
}

// DefaultComplexityThresholds returns sensible defaults.
func DefaultComplexityThresholds() ComplexityThresholds {
	return ComplexityThresholds{
		MaxCyclomatic: 10,
		MaxCognitive:  15,
		MaxNesting:    4,
	}
}

// ExtendedComplexityThresholds provides warn and error levels (pmat compatible).
type ExtendedComplexityThresholds struct {
	CyclomaticWarn  uint32 `json:"cyclomatic_warn"`
	CyclomaticError uint32 `json:"cyclomatic_error"`
	CognitiveWarn   uint32 `json:"cognitive_warn"`
	CognitiveError  uint32 `json:"cognitive_error"`
	NestingMax      uint8  `json:"nesting_max"`
	MethodLength    uint16 `json:"method_length"`
}

// DefaultExtendedThresholds returns pmat-compatible default thresholds.
func DefaultExtendedThresholds() ExtendedComplexityThresholds {
	return ExtendedComplexityThresholds{
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

// ComplexityHotspot identifies a high-complexity location in the codebase.
type ComplexityHotspot struct {
	File           string `json:"file"`
	Function       string `json:"function,omitempty"`
	Line           uint32 `json:"line"`
	Complexity     uint32 `json:"complexity"`
	ComplexityType string `json:"complexity_type"`
}

// ComplexityReport is the full analysis report with violations and hotspots.
type ComplexityReport struct {
	Summary            ExtendedComplexitySummary `json:"summary"`
	Violations         []Violation               `json:"violations"`
	Hotspots           []ComplexityHotspot       `json:"hotspots"`
	Files              []FileComplexity          `json:"files"`
	TechnicalDebtHours float32                   `json:"technical_debt_hours"`
}

// ExtendedComplexitySummary provides enhanced statistics (pmat compatible).
type ExtendedComplexitySummary struct {
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
func (m *ComplexityMetrics) IsSimple(t ComplexityThresholds) bool {
	return m.Cyclomatic <= t.MaxCyclomatic &&
		m.Cognitive <= t.MaxCognitive &&
		m.MaxNesting <= t.MaxNesting
}

// NeedsRefactoring returns true if any metric significantly exceeds thresholds.
func (m *ComplexityMetrics) NeedsRefactoring(t ComplexityThresholds) bool {
	return m.Cyclomatic > t.MaxCyclomatic*2 ||
		m.Cognitive > t.MaxCognitive*2 ||
		m.MaxNesting > t.MaxNesting*2
}

// IsSimpleDefault checks if complexity is low using fixed thresholds (pmat compatible).
// Returns true if cyclomatic <= 5 and cognitive <= 7.
func (m *ComplexityMetrics) IsSimpleDefault() bool {
	return m.Cyclomatic <= 5 && m.Cognitive <= 7
}

// NeedsRefactoringDefault checks if complexity exceeds fixed thresholds (pmat compatible).
// Returns true if cyclomatic > 10 or cognitive > 15.
func (m *ComplexityMetrics) NeedsRefactoringDefault() bool {
	return m.Cyclomatic > 10 || m.Cognitive > 15
}

// ComplexityScore calculates a composite complexity score for ranking.
// Combines cyclomatic, cognitive, nesting, and lines with weighted factors.
func (m *ComplexityMetrics) ComplexityScore() float64 {
	return float64(m.Cyclomatic)*1.0 +
		float64(m.Cognitive)*1.2 +
		float64(m.MaxNesting)*2.0 +
		float64(m.Lines)*0.1
}
