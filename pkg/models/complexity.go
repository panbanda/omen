package models

// ComplexityMetrics represents code complexity measurements for a function or file.
type ComplexityMetrics struct {
	Cyclomatic uint32           `json:"cyclomatic"`
	Cognitive  uint32           `json:"cognitive"`
	MaxNesting int              `json:"max_nesting"`
	Lines      int              `json:"lines"`
	Halstead   *HalsteadMetrics `json:"halstead,omitempty"`
}

// HalsteadMetrics represents Halstead software science metrics.
type HalsteadMetrics struct {
	OperatorsUnique uint16  `json:"operators_unique"`
	OperandsUnique  uint16  `json:"operands_unique"`
	OperatorsTotal  uint16  `json:"operators_total"`
	OperandsTotal   uint16  `json:"operands_total"`
	Volume          float64 `json:"volume"`
	Difficulty      float64 `json:"difficulty"`
	Effort          float64 `json:"effort"`
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
	P95Cyclomatic  uint32  `json:"p95_cyclomatic"`
	P50Cognitive   uint32  `json:"p50_cognitive"`
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
