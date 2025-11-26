package models

import "time"

// DebtCategory represents the type of technical debt.
type DebtCategory string

const (
	DebtDesign      DebtCategory = "design"      // HACK, KLUDGE, SMELL
	DebtDefect      DebtCategory = "defect"      // BUG, FIXME, BROKEN
	DebtRequirement DebtCategory = "requirement" // TODO, FEAT, ENHANCEMENT
	DebtTest        DebtCategory = "test"        // FAILING, SKIP, DISABLED
	DebtPerformance DebtCategory = "performance" // SLOW, OPTIMIZE, PERF
	DebtSecurity    DebtCategory = "security"    // SECURITY, VULN, UNSAFE
)

// Severity represents the urgency of addressing the debt.
type Severity string

const (
	SeverityLow      Severity = "low"
	SeverityMedium   Severity = "medium"
	SeverityHigh     Severity = "high"
	SeverityCritical Severity = "critical"
)

// TechnicalDebt represents a single SATD item found in code.
type TechnicalDebt struct {
	Category    DebtCategory `json:"category"`
	Severity    Severity     `json:"severity"`
	File        string       `json:"file"`
	Line        uint32       `json:"line"`
	Description string       `json:"description"`
	Marker      string       `json:"marker"` // TODO, FIXME, HACK, etc.
	Text        string       `json:"text,omitempty"`
	Column      uint32       `json:"column,omitempty"`
	ContextHash string       `json:"context_hash,omitempty"` // BLAKE3 hash for identity tracking
	Author      string       `json:"author,omitempty"`
	Date        *time.Time   `json:"date,omitempty"`
}

// SATDAnalysis represents the full SATD analysis result.
type SATDAnalysis struct {
	Items              []TechnicalDebt `json:"items"`
	Summary            SATDSummary     `json:"summary"`
	TotalFilesAnalyzed int             `json:"total_files_analyzed"`
	FilesWithDebt      int             `json:"files_with_debt"`
	AnalyzedAt         time.Time       `json:"analyzed_at"`
}

// SATDSummary provides aggregate statistics.
type SATDSummary struct {
	TotalItems int            `json:"total_items"`
	BySeverity map[string]int `json:"by_severity"`
	ByCategory map[string]int `json:"by_category"`
	ByFile     map[string]int `json:"by_file"`
}

// NewSATDSummary creates an initialized summary.
func NewSATDSummary() SATDSummary {
	return SATDSummary{
		BySeverity: make(map[string]int),
		ByCategory: make(map[string]int),
		ByFile:     make(map[string]int),
	}
}

// AddItem updates the summary with a new debt item.
func (s *SATDSummary) AddItem(item TechnicalDebt) {
	s.TotalItems++
	s.BySeverity[string(item.Severity)]++
	s.ByCategory[string(item.Category)]++
	s.ByFile[item.File]++
}

// SeverityWeight returns a numeric weight for sorting.
func (s Severity) Weight() int {
	switch s {
	case SeverityCritical:
		return 4
	case SeverityHigh:
		return 3
	case SeverityMedium:
		return 2
	case SeverityLow:
		return 1
	default:
		return 0
	}
}
