package satd

import "time"

// Category represents the type of technical debt. omen:ignore
type Category string

// String implements fmt.Stringer for json serialization.
func (d Category) String() string {
	return string(d)
}

const (
	CategoryDesign      Category = "design"      // HACK, KLUDGE, SMELL
	CategoryDefect      Category = "defect"      // BUG, FIXME, BROKEN
	CategoryRequirement Category = "requirement" // TODO, FEAT, ENHANCEMENT
	CategoryTest        Category = "test"        // FAILING, SKIP, DISABLED
	CategoryPerformance Category = "performance" // SLOW, OPTIMIZE, PERF
	CategorySecurity    Category = "security"    // SECURITY, VULN, UNSAFE
)

// Severity represents the urgency of addressing the debt.
type Severity string

// String implements fmt.Stringer for json serialization.
func (s Severity) String() string {
	return string(s)
}

const (
	SeverityLow      Severity = "low"
	SeverityMedium   Severity = "medium"
	SeverityHigh     Severity = "high"
	SeverityCritical Severity = "critical"
)

// Item represents a single SATD item found in code.
type Item struct {
	Category    Category   `json:"category"`
	Severity    Severity   `json:"severity"`
	File        string     `json:"file"`
	Line        uint32     `json:"line"`
	Description string     `json:"description"`
	Marker      string     `json:"marker"` // TODO, FIXME, HACK, etc.
	Text        string     `json:"text,omitempty"`
	Column      uint32     `json:"column,omitempty"`
	ContextHash string     `json:"context_hash,omitempty"` // BLAKE3 hash for identity tracking
	Author      string     `json:"author,omitempty"`
	Date        *time.Time `json:"date,omitempty"`
}

// Analysis represents the full SATD analysis result.
type Analysis struct {
	Items              []Item    `json:"items"`
	Summary            Summary   `json:"summary"`
	TotalFilesAnalyzed int       `json:"total_files_analyzed"`
	FilesWithDebt      int       `json:"files_with_debt"`
	AnalyzedAt         time.Time `json:"analyzed_at"`
}

// Summary provides aggregate statistics.
type Summary struct {
	TotalItems    int            `json:"total_items"`
	BySeverity    map[string]int `json:"by_severity"`
	ByCategory    map[string]int `json:"by_category"`
	ByFile        map[string]int `json:"by_file,omitempty"`
	FilesWithSATD int            `json:"files_with_satd,omitempty"`
	AvgAgeDays    float64        `json:"avg_age_days,omitempty"`
}

// NewSummary creates an initialized summary.
func NewSummary() Summary {
	return Summary{
		BySeverity: make(map[string]int),
		ByCategory: make(map[string]int),
		ByFile:     make(map[string]int),
	}
}

// AddItem updates the summary with a new debt item.
func (s *Summary) AddItem(item Item) {
	s.TotalItems++
	s.BySeverity[string(item.Severity)]++
	s.ByCategory[string(item.Category)]++
	s.ByFile[item.File]++
}

// Weight returns a numeric weight for sorting.
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

// Escalate increases severity by one level (max Critical).
func (s Severity) Escalate() Severity {
	switch s {
	case SeverityLow:
		return SeverityMedium
	case SeverityMedium:
		return SeverityHigh
	case SeverityHigh:
		return SeverityCritical
	default:
		return s
	}
}

// Reduce decreases severity by one level (min Low).
func (s Severity) Reduce() Severity {
	switch s {
	case SeverityCritical:
		return SeverityHigh
	case SeverityHigh:
		return SeverityMedium
	case SeverityMedium:
		return SeverityLow
	default:
		return s
	}
}

// AstNodeType represents the type of AST context for a code location.
// Used for severity adjustment based on context.
type AstNodeType int

const (
	// AstNodeRegular is a normal code location with no special context.
	AstNodeRegular AstNodeType = iota
	// AstNodeSecurityFunction is code within a security-related function. omen:ignore
	AstNodeSecurityFunction
	// AstNodeDataValidation is code within a data validation function.
	AstNodeDataValidation
	// AstNodeTestFunction is code within a test function. omen:ignore
	AstNodeTestFunction
	// AstNodeMockImplementation is code within a mock/stub implementation.
	AstNodeMockImplementation
)

// AstContext provides context about a code location for severity adjustment.
type AstContext struct {
	NodeType              AstNodeType
	ParentFunction        string
	Complexity            uint32
	SiblingsCount         int
	NestingDepth          int
	SurroundingStatements []string
}
