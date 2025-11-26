package models

// DeadFunction represents an unused function detected in the codebase.
type DeadFunction struct {
	Name       string  `json:"name"`
	File       string  `json:"file"`
	Line       uint32  `json:"line"`
	EndLine    uint32  `json:"end_line"`
	Visibility string  `json:"visibility"` // public, private, internal
	Confidence float64 `json:"confidence"` // 0.0-1.0, how certain we are it's dead
	Reason     string  `json:"reason"`     // Why it's considered dead
}

// DeadCodeAnalysis represents the full dead code detection result.
type DeadCodeAnalysis struct {
	DeadFunctions   []DeadFunction     `json:"dead_functions"`
	DeadVariables   []DeadVariable     `json:"dead_variables"`
	UnreachableCode []UnreachableBlock `json:"unreachable_code"`
	Summary         DeadCodeSummary    `json:"summary"`
}

// DeadVariable represents an unused variable.
type DeadVariable struct {
	Name       string  `json:"name"`
	File       string  `json:"file"`
	Line       uint32  `json:"line"`
	Confidence float64 `json:"confidence"`
}

// UnreachableBlock represents code that can never execute.
type UnreachableBlock struct {
	File      string `json:"file"`
	StartLine uint32 `json:"start_line"`
	EndLine   uint32 `json:"end_line"`
	Reason    string `json:"reason"` // e.g., "after return", "dead branch"
}

// DeadCodeSummary provides aggregate statistics.
type DeadCodeSummary struct {
	TotalDeadFunctions    int            `json:"total_dead_functions"`
	TotalDeadVariables    int            `json:"total_dead_variables"`
	TotalUnreachableLines int            `json:"total_unreachable_lines"`
	DeadCodePercentage    float64        `json:"dead_code_percentage"`
	ByFile                map[string]int `json:"by_file"`
	TotalFilesAnalyzed    int            `json:"total_files_analyzed"`
	TotalLinesAnalyzed    int            `json:"total_lines_analyzed"`
}

// NewDeadCodeSummary creates an initialized summary.
func NewDeadCodeSummary() DeadCodeSummary {
	return DeadCodeSummary{
		ByFile: make(map[string]int),
	}
}

// AddDeadFunction updates the summary with a dead function.
func (s *DeadCodeSummary) AddDeadFunction(f DeadFunction) {
	s.TotalDeadFunctions++
	s.ByFile[f.File]++
}

// AddDeadVariable updates the summary with a dead variable.
func (s *DeadCodeSummary) AddDeadVariable(v DeadVariable) {
	s.TotalDeadVariables++
	s.ByFile[v.File]++
}

// AddUnreachableBlock updates the summary with unreachable code.
func (s *DeadCodeSummary) AddUnreachableBlock(b UnreachableBlock) {
	lines := int(b.EndLine - b.StartLine + 1)
	s.TotalUnreachableLines += lines
	s.ByFile[b.File] += lines
}

// CalculatePercentage computes dead code percentage.
func (s *DeadCodeSummary) CalculatePercentage() {
	if s.TotalLinesAnalyzed > 0 {
		deadLines := s.TotalUnreachableLines
		// Estimate ~10 lines per dead function
		deadLines += s.TotalDeadFunctions * 10
		// Estimate ~1 line per dead variable
		deadLines += s.TotalDeadVariables
		s.DeadCodePercentage = float64(deadLines) / float64(s.TotalLinesAnalyzed) * 100
	}
}
