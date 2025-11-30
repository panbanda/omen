package models

import "time"

// ConfidenceLevel indicates how certain we are about dead code detection.
type ConfidenceLevel string

const (
	ConfidenceHigh   ConfidenceLevel = "High"
	ConfidenceMedium ConfidenceLevel = "Medium"
	ConfidenceLow    ConfidenceLevel = "Low"
)

// DeadCodeType classifies the type of dead code item (pmat compatible).
type DeadCodeType string

const (
	DeadCodeTypeFunction    DeadCodeType = "function"
	DeadCodeTypeClass       DeadCodeType = "class"
	DeadCodeTypeVariable    DeadCodeType = "variable"
	DeadCodeTypeUnreachable DeadCodeType = "unreachable"
)

// DeadCodeItem represents an individual dead code item within a file (pmat compatible).
type DeadCodeItem struct {
	ItemType DeadCodeType `json:"item_type"`
	Name     string       `json:"name"`
	Line     uint32       `json:"line"`
	Reason   string       `json:"reason"`
}

// FileDeadCodeMetrics contains file-level dead code metrics with items (pmat compatible).
type FileDeadCodeMetrics struct {
	Path              string          `json:"path"`
	DeadLines         int             `json:"dead_lines"`
	TotalLines        int             `json:"total_lines"`
	DeadPercentage    float32         `json:"dead_percentage"`
	DeadFunctions     int             `json:"dead_functions"`
	DeadClasses       int             `json:"dead_classes"`
	DeadModules       int             `json:"dead_modules"`
	UnreachableBlocks int             `json:"unreachable_blocks"`
	DeadScore         float32         `json:"dead_score"`
	Confidence        ConfidenceLevel `json:"confidence"`
	Items             []DeadCodeItem  `json:"items"`
}

// CalculateScore calculates the dead code score using weighted algorithm (pmat compatible).
func (f *FileDeadCodeMetrics) CalculateScore() {
	percentageWeight := float32(0.35)
	absoluteWeight := float32(0.30)
	functionWeight := float32(0.20)
	confidenceWeight := float32(0.15)

	var confidenceMultiplier float32
	switch f.Confidence {
	case ConfidenceHigh:
		confidenceMultiplier = 1.0
	case ConfidenceMedium:
		confidenceMultiplier = 0.7
	case ConfidenceLow:
		confidenceMultiplier = 0.4
	default:
		confidenceMultiplier = 0.7
	}

	deadLines := f.DeadLines
	if deadLines > 1000 {
		deadLines = 1000
	}

	deadFunctions := f.DeadFunctions
	if deadFunctions > 50 {
		deadFunctions = 50
	}

	f.DeadScore = (f.DeadPercentage * percentageWeight) +
		(float32(deadLines) / 10.0 * absoluteWeight) +
		(float32(deadFunctions) * 2.0 * functionWeight) +
		(100.0 * confidenceMultiplier * confidenceWeight)
}

// UpdatePercentage updates the dead percentage based on current counts.
func (f *FileDeadCodeMetrics) UpdatePercentage() {
	if f.TotalLines > 0 {
		f.DeadPercentage = float32(f.DeadLines) / float32(f.TotalLines) * 100.0
	}
}

// AddItem adds a dead code item and updates counts.
func (f *FileDeadCodeMetrics) AddItem(item DeadCodeItem) {
	switch item.ItemType {
	case DeadCodeTypeFunction:
		f.DeadFunctions++
		f.DeadLines += 10 // Estimate 10 lines per function
	case DeadCodeTypeClass:
		f.DeadClasses++
		f.DeadLines += 10 // Estimate 10 lines per class
	case DeadCodeTypeVariable:
		f.DeadModules++  // Variables tracked in modules counter
		f.DeadLines += 1 // Estimate 1 line per variable
	case DeadCodeTypeUnreachable:
		f.UnreachableBlocks++
		f.DeadLines += 3 // Estimate 3 lines per unreachable block
	}
	f.Items = append(f.Items, item)
}

// DeadCodeRankingSummary provides aggregate statistics (pmat compatible).
type DeadCodeRankingSummary struct {
	TotalFilesAnalyzed int     `json:"total_files_analyzed"`
	FilesWithDeadCode  int     `json:"files_with_dead_code"`
	TotalDeadLines     int     `json:"total_dead_lines"`
	DeadPercentage     float32 `json:"dead_percentage"`
	DeadFunctions      int     `json:"dead_functions"`
	DeadClasses        int     `json:"dead_classes"`
	DeadModules        int     `json:"dead_modules"`
	UnreachableBlocks  int     `json:"unreachable_blocks"`
}

// DeadCodeAnalysisConfig holds configuration for dead code analysis (pmat compatible).
type DeadCodeAnalysisConfig struct {
	IncludeUnreachable bool `json:"include_unreachable"`
	IncludeTests       bool `json:"include_tests"`
	MinDeadLines       int  `json:"min_dead_lines"`
}

// DeadCodeResult is the pmat-compatible output format for dead code analysis.
type DeadCodeResult struct {
	Summary           DeadCodeRankingSummary `json:"summary"`
	Files             []FileDeadCodeMetrics  `json:"files"`
	TotalFiles        int                    `json:"total_files"`
	AnalyzedFiles     int                    `json:"analyzed_files"`
	AnalysisTimestamp time.Time              `json:"analysis_timestamp,omitempty"`
	Config            DeadCodeAnalysisConfig `json:"config,omitempty"`
}

// NewDeadCodeResult creates a new pmat-compatible dead code result.
func NewDeadCodeResult() *DeadCodeResult {
	return &DeadCodeResult{
		Files: make([]FileDeadCodeMetrics, 0),
		Config: DeadCodeAnalysisConfig{
			IncludeUnreachable: false,
			IncludeTests:       false,
			MinDeadLines:       10,
		},
	}
}

// FromDeadCodeAnalysis converts the internal DeadCodeAnalysis to pmat-compatible format.
func (r *DeadCodeResult) FromDeadCodeAnalysis(analysis *DeadCodeAnalysis) {
	// Group items by file
	fileMap := make(map[string]*FileDeadCodeMetrics)

	// Process dead functions
	for _, df := range analysis.DeadFunctions {
		fm := r.getOrCreateFileMetrics(fileMap, df.File)
		fm.AddItem(DeadCodeItem{
			ItemType: DeadCodeTypeFunction,
			Name:     df.Name,
			Line:     df.Line,
			Reason:   df.Reason,
		})
		// Determine confidence from item confidence
		if df.Confidence >= 0.9 {
			fm.Confidence = ConfidenceHigh
		} else if df.Confidence >= 0.6 {
			fm.Confidence = ConfidenceMedium
		} else {
			fm.Confidence = ConfidenceLow
		}
	}

	// Process dead classes
	for _, dc := range analysis.DeadClasses {
		fm := r.getOrCreateFileMetrics(fileMap, dc.File)
		fm.AddItem(DeadCodeItem{
			ItemType: DeadCodeTypeClass,
			Name:     dc.Name,
			Line:     dc.Line,
			Reason:   dc.Reason,
		})
	}

	// Process dead variables
	for _, dv := range analysis.DeadVariables {
		fm := r.getOrCreateFileMetrics(fileMap, dv.File)
		fm.AddItem(DeadCodeItem{
			ItemType: DeadCodeTypeVariable,
			Name:     dv.Name,
			Line:     dv.Line,
			Reason:   dv.Reason,
		})
	}

	// Process unreachable blocks
	for _, ub := range analysis.UnreachableCode {
		fm := r.getOrCreateFileMetrics(fileMap, ub.File)
		fm.AddItem(DeadCodeItem{
			ItemType: DeadCodeTypeUnreachable,
			Name:     "unreachable_block",
			Line:     ub.StartLine,
			Reason:   ub.Reason,
		})
	}

	// Convert map to slice and calculate scores
	for _, fm := range fileMap {
		fm.UpdatePercentage()
		fm.CalculateScore()
		r.Files = append(r.Files, *fm)
	}

	// Sort files by dead score descending
	r.sortFilesByScore()

	// Build summary
	r.buildSummary(analysis)

	r.TotalFiles = analysis.Summary.TotalFilesAnalyzed
	r.AnalyzedFiles = analysis.Summary.TotalFilesAnalyzed
	r.AnalysisTimestamp = time.Now().UTC()
}

func (r *DeadCodeResult) getOrCreateFileMetrics(fileMap map[string]*FileDeadCodeMetrics, path string) *FileDeadCodeMetrics {
	if fm, exists := fileMap[path]; exists {
		return fm
	}
	fm := &FileDeadCodeMetrics{
		Path:       path,
		Confidence: ConfidenceMedium,
		Items:      make([]DeadCodeItem, 0),
	}
	fileMap[path] = fm
	return fm
}

func (r *DeadCodeResult) sortFilesByScore() {
	// Simple bubble sort for now (files list is typically small)
	for i := 0; i < len(r.Files)-1; i++ {
		for j := 0; j < len(r.Files)-i-1; j++ {
			if r.Files[j].DeadScore < r.Files[j+1].DeadScore {
				r.Files[j], r.Files[j+1] = r.Files[j+1], r.Files[j]
			}
		}
	}
}

func (r *DeadCodeResult) buildSummary(analysis *DeadCodeAnalysis) {
	r.Summary.TotalFilesAnalyzed = analysis.Summary.TotalFilesAnalyzed
	r.Summary.FilesWithDeadCode = len(r.Files)
	r.Summary.DeadFunctions = analysis.Summary.TotalDeadFunctions
	r.Summary.DeadClasses = analysis.Summary.TotalDeadClasses
	r.Summary.DeadModules = analysis.Summary.TotalDeadVariables
	r.Summary.UnreachableBlocks = analysis.Summary.TotalUnreachableBlocks

	// Sum dead lines from file metrics
	totalDeadLines := 0
	totalLines := 0
	for _, f := range r.Files {
		totalDeadLines += f.DeadLines
		totalLines += f.TotalLines
	}
	r.Summary.TotalDeadLines = totalDeadLines

	if totalLines > 0 {
		r.Summary.DeadPercentage = float32(totalDeadLines) / float32(totalLines) * 100.0
	} else {
		r.Summary.DeadPercentage = float32(analysis.Summary.DeadCodePercentage)
	}
}

// ReferenceType classifies the relationship between code elements.
type ReferenceType string

const (
	RefDirectCall      ReferenceType = "direct_call"
	RefIndirectCall    ReferenceType = "indirect_call"
	RefImport          ReferenceType = "import"
	RefInheritance     ReferenceType = "inheritance"
	RefTypeReference   ReferenceType = "type_reference"
	RefDynamicDispatch ReferenceType = "dynamic_dispatch"
)

// DeadCodeKind classifies the type of dead code detected.
type DeadCodeKind string

const (
	DeadKindFunction    DeadCodeKind = "unused_function"
	DeadKindClass       DeadCodeKind = "unused_class"
	DeadKindVariable    DeadCodeKind = "unused_variable"
	DeadKindUnreachable DeadCodeKind = "unreachable_code"
	DeadKindDeadBranch  DeadCodeKind = "dead_branch"
)

// ReferenceEdge represents a relationship between two code elements.
type ReferenceEdge struct {
	From       uint32        `json:"from"`
	To         uint32        `json:"to"`
	Type       ReferenceType `json:"type"`
	Confidence float64       `json:"confidence"`
}

// ReferenceNode represents a code element in the reference graph.
type ReferenceNode struct {
	ID         uint32 `json:"id"`
	Name       string `json:"name"`
	File       string `json:"file"`
	Line       uint32 `json:"line"`
	EndLine    uint32 `json:"end_line"`
	Kind       string `json:"kind"` // function, class, variable
	Language   string `json:"language"`
	IsExported bool   `json:"is_exported"`
	IsEntry    bool   `json:"is_entry"`
}

// CallGraph represents the reference graph for reachability analysis.
type CallGraph struct {
	Nodes       map[uint32]*ReferenceNode `json:"nodes" toon:"-"`
	Edges       []ReferenceEdge           `json:"edges" toon:"edges"`
	EntryPoints []uint32                  `json:"entry_points" toon:"entry_points"`
	EdgeIndex   map[uint32][]int          `json:"-" toon:"-"` // node -> edge indices (outgoing)
}

// NewCallGraph creates an initialized call graph.
func NewCallGraph() *CallGraph {
	return &CallGraph{
		Nodes:       make(map[uint32]*ReferenceNode),
		Edges:       make([]ReferenceEdge, 0),
		EntryPoints: make([]uint32, 0),
		EdgeIndex:   make(map[uint32][]int),
	}
}

// AddNode adds a node to the call graph.
func (g *CallGraph) AddNode(node *ReferenceNode) {
	g.Nodes[node.ID] = node
	if node.IsEntry {
		g.EntryPoints = append(g.EntryPoints, node.ID)
	}
}

// AddEdge adds an edge to the call graph with indexing.
func (g *CallGraph) AddEdge(edge ReferenceEdge) {
	edgeIdx := len(g.Edges)
	g.Edges = append(g.Edges, edge)
	g.EdgeIndex[edge.From] = append(g.EdgeIndex[edge.From], edgeIdx)
}

// GetOutgoingEdges returns all edges originating from a node.
func (g *CallGraph) GetOutgoingEdges(nodeID uint32) []ReferenceEdge {
	indices := g.EdgeIndex[nodeID]
	edges := make([]ReferenceEdge, len(indices))
	for i, idx := range indices {
		edges[i] = g.Edges[idx]
	}
	return edges
}

// DeadFunction represents an unused function detected in the codebase.
type DeadFunction struct {
	Name             string          `json:"name"`
	File             string          `json:"file"`
	Line             uint32          `json:"line"`
	EndLine          uint32          `json:"end_line"`
	Visibility       string          `json:"visibility"` // public, private, internal
	Confidence       float64         `json:"confidence"` // 0.0-1.0, how certain we are it's dead
	ConfidenceLevel  ConfidenceLevel `json:"confidence_level"`
	ConfidenceReason string          `json:"confidence_reason"` // Why we have this confidence level
	Reason           string          `json:"reason"`            // Why it's considered dead
	Kind             DeadCodeKind    `json:"kind,omitempty"`
	NodeID           uint32          `json:"node_id,omitempty"`
}

// SetConfidenceLevel sets the confidence level and reason based on the numeric confidence.
func (f *DeadFunction) SetConfidenceLevel() {
	if f.Confidence >= 0.8 {
		f.ConfidenceLevel = ConfidenceHigh
		f.ConfidenceReason = "High confidence: private/unexported, no references in call graph"
	} else if f.Confidence >= 0.5 {
		f.ConfidenceLevel = ConfidenceMedium
		f.ConfidenceReason = "Medium confidence: exported but no internal references found"
	} else {
		f.ConfidenceLevel = ConfidenceLow
		f.ConfidenceReason = "Low confidence: matches patterns that may have dynamic usage"
	}
}

// DeadCodeAnalysis represents the full dead code detection result.
type DeadCodeAnalysis struct {
	DeadFunctions   []DeadFunction     `json:"dead_functions"`
	DeadVariables   []DeadVariable     `json:"dead_variables"`
	DeadClasses     []DeadClass        `json:"dead_classes,omitempty"`
	UnreachableCode []UnreachableBlock `json:"unreachable_code"`
	Summary         DeadCodeSummary    `json:"summary"`
	CallGraph       *CallGraph         `json:"call_graph,omitempty"`
}

// DeadClass represents an unused class/struct/type.
type DeadClass struct {
	Name             string          `json:"name"`
	File             string          `json:"file"`
	Line             uint32          `json:"line"`
	EndLine          uint32          `json:"end_line"`
	Visibility       string          `json:"visibility"`
	Confidence       float64         `json:"confidence"`
	ConfidenceLevel  ConfidenceLevel `json:"confidence_level"`
	ConfidenceReason string          `json:"confidence_reason"`
	Reason           string          `json:"reason"`
	Kind             DeadCodeKind    `json:"kind,omitempty"`
	NodeID           uint32          `json:"node_id,omitempty"`
}

// SetConfidenceLevel sets the confidence level and reason based on the numeric confidence.
func (c *DeadClass) SetConfidenceLevel() {
	if c.Confidence >= 0.8 {
		c.ConfidenceLevel = ConfidenceHigh
		c.ConfidenceReason = "High confidence: private/unexported type, no references in codebase"
	} else if c.Confidence >= 0.5 {
		c.ConfidenceLevel = ConfidenceMedium
		c.ConfidenceReason = "Medium confidence: exported type but no internal usages found"
	} else {
		c.ConfidenceLevel = ConfidenceLow
		c.ConfidenceReason = "Low confidence: may be used via reflection or as public API"
	}
}

// DeadVariable represents an unused variable.
type DeadVariable struct {
	Name             string          `json:"name"`
	File             string          `json:"file"`
	Line             uint32          `json:"line"`
	Visibility       string          `json:"visibility"`
	Confidence       float64         `json:"confidence"`
	ConfidenceLevel  ConfidenceLevel `json:"confidence_level"`
	ConfidenceReason string          `json:"confidence_reason"`
	Reason           string          `json:"reason,omitempty"`
	Kind             DeadCodeKind    `json:"kind,omitempty"`
	NodeID           uint32          `json:"node_id,omitempty"`
}

// SetConfidenceLevel sets the confidence level and reason based on the numeric confidence.
func (v *DeadVariable) SetConfidenceLevel() {
	if v.Confidence >= 0.8 {
		v.ConfidenceLevel = ConfidenceHigh
		v.ConfidenceReason = "High confidence: private/unexported variable, no references found"
	} else if v.Confidence >= 0.5 {
		v.ConfidenceLevel = ConfidenceMedium
		v.ConfidenceReason = "Medium confidence: exported variable but no internal usages found"
	} else {
		v.ConfidenceLevel = ConfidenceLow
		v.ConfidenceReason = "Low confidence: may be accessed dynamically or via reflection"
	}
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
	TotalDeadFunctions     int                  `json:"total_dead_functions" toon:"total_dead_functions"`
	TotalDeadVariables     int                  `json:"total_dead_variables" toon:"total_dead_variables"`
	TotalDeadClasses       int                  `json:"total_dead_classes" toon:"total_dead_classes"`
	TotalUnreachableBlocks int                  `json:"total_unreachable_blocks" toon:"total_unreachable_blocks"`
	TotalUnreachableLines  int                  `json:"total_unreachable_lines" toon:"total_unreachable_lines"`
	DeadCodePercentage     float64              `json:"dead_code_percentage" toon:"dead_code_percentage"`
	ByFile                 map[string]int       `json:"by_file" toon:"by_file"`
	ByKind                 map[DeadCodeKind]int `json:"by_kind,omitempty" toon:"-"`
	TotalFilesAnalyzed     int                  `json:"total_files_analyzed" toon:"total_files_analyzed"`
	TotalLinesAnalyzed     int                  `json:"total_lines_analyzed" toon:"total_lines_analyzed"`
	TotalNodesInGraph      int                  `json:"total_nodes_in_graph,omitempty" toon:"total_nodes_in_graph,omitempty"`
	ReachableNodes         int                  `json:"reachable_nodes,omitempty" toon:"reachable_nodes,omitempty"`
	UnreachableNodes       int                  `json:"unreachable_nodes,omitempty" toon:"unreachable_nodes,omitempty"`
	ConfidenceLevel        float64              `json:"confidence_level,omitempty" toon:"confidence_level,omitempty"`
}

// NewDeadCodeSummary creates an initialized summary.
func NewDeadCodeSummary() DeadCodeSummary {
	return DeadCodeSummary{
		ByFile: make(map[string]int),
		ByKind: make(map[DeadCodeKind]int),
	}
}

// AddDeadFunction updates the summary with a dead function.
func (s *DeadCodeSummary) AddDeadFunction(f DeadFunction) {
	s.TotalDeadFunctions++
	s.ByFile[f.File]++
	if f.Kind != "" {
		s.ByKind[f.Kind]++
	} else {
		s.ByKind[DeadKindFunction]++
	}
}

// AddDeadVariable updates the summary with a dead variable.
func (s *DeadCodeSummary) AddDeadVariable(v DeadVariable) {
	s.TotalDeadVariables++
	s.ByFile[v.File]++
	if v.Kind != "" {
		s.ByKind[v.Kind]++
	} else {
		s.ByKind[DeadKindVariable]++
	}
}

// AddDeadClass updates the summary with a dead class.
func (s *DeadCodeSummary) AddDeadClass(c DeadClass) {
	s.TotalDeadClasses++
	s.ByFile[c.File]++
	if c.Kind != "" {
		s.ByKind[c.Kind]++
	} else {
		s.ByKind[DeadKindClass]++
	}
}

// AddUnreachableBlock updates the summary with unreachable code.
func (s *DeadCodeSummary) AddUnreachableBlock(b UnreachableBlock) {
	lines := int(b.EndLine - b.StartLine + 1)
	s.TotalUnreachableLines += lines
	s.ByFile[b.File] += lines
	s.ByKind[DeadKindUnreachable] += lines
}

// CalculatePercentage computes dead code percentage.
func (s *DeadCodeSummary) CalculatePercentage() {
	if s.TotalLinesAnalyzed > 0 {
		deadLines := s.TotalUnreachableLines
		// Estimate ~10 lines per dead function
		deadLines += s.TotalDeadFunctions * 10
		// Estimate ~5 lines per dead class
		deadLines += s.TotalDeadClasses * 5
		// Estimate ~1 line per dead variable
		deadLines += s.TotalDeadVariables
		s.DeadCodePercentage = float64(deadLines) / float64(s.TotalLinesAnalyzed) * 100
	}

	// Calculate from graph if available
	if s.TotalNodesInGraph > 0 {
		s.UnreachableNodes = s.TotalNodesInGraph - s.ReachableNodes
		if s.DeadCodePercentage == 0 {
			s.DeadCodePercentage = float64(s.UnreachableNodes) / float64(s.TotalNodesInGraph) * 100
		}
	}
}
