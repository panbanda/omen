package models

import "math"

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
	OperatorsUnique uint32  `json:"operators_unique"` // n1: distinct operators
	OperandsUnique  uint32  `json:"operands_unique"`  // n2: distinct operands
	OperatorsTotal  uint32  `json:"operators_total"`  // N1: total operators
	OperandsTotal   uint32  `json:"operands_total"`   // N2: total operands
	Vocabulary      uint32  `json:"vocabulary"`       // n = n1 + n2
	Length          uint32  `json:"length"`           // N = N1 + N2
	Volume          float64 `json:"volume"`           // V = N * log2(n)
	Difficulty      float64 `json:"difficulty"`       // D = (n1/2) * (N2/n2)
	Effort          float64 `json:"effort"`           // E = D * V
	Time            float64 `json:"time"`             // T = E / 18 (seconds)
	Bugs            float64 `json:"bugs"`             // B = E^(2/3) / 3000
}

// NewHalsteadMetrics creates Halstead metrics from base counts and calculates derived values.
func NewHalsteadMetrics(operatorsUnique, operandsUnique, operatorsTotal, operandsTotal uint32) *HalsteadMetrics {
	h := &HalsteadMetrics{
		OperatorsUnique: operatorsUnique,
		OperandsUnique:  operandsUnique,
		OperatorsTotal:  operatorsTotal,
		OperandsTotal:   operandsTotal,
	}
	h.calculateDerived()
	return h
}

// calculateDerived computes all derived Halstead metrics from base counts.
func (h *HalsteadMetrics) calculateDerived() {
	if h.OperatorsUnique == 0 || h.OperandsUnique == 0 {
		return
	}

	h.Vocabulary = h.OperatorsUnique + h.OperandsUnique
	h.Length = h.OperatorsTotal + h.OperandsTotal

	// V = N * log2(n) - Program Volume
	if h.Vocabulary > 0 {
		h.Volume = float64(h.Length) * log2(float64(h.Vocabulary))
	}

	// D = (n1/2) * (N2/n2) - Program Difficulty
	if h.OperandsUnique > 0 {
		h.Difficulty = (float64(h.OperatorsUnique) / 2.0) *
			(float64(h.OperandsTotal) / float64(h.OperandsUnique))
	}

	// E = V * D - Programming Effort
	h.Effort = h.Volume * h.Difficulty

	// T = E / 18 - Time to program in seconds (18 mental discriminations per second)
	h.Time = h.Effort / 18.0

	// B = E^(2/3) / 3000 - Delivered bugs estimate
	h.Bugs = pow(h.Effort, 2.0/3.0) / 3000.0
}

// log2 computes log base 2
func log2(x float64) float64 {
	if x <= 0 {
		return 0
	}
	return math.Log2(x)
}

// pow computes x^y
func pow(x, y float64) float64 {
	if x <= 0 {
		return 0
	}
	return math.Pow(x, y)
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
