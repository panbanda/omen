package deadcode

import (
	"sync"
	"time"
)

// ConfidenceLevel indicates how certain we are about dead code detection.
type ConfidenceLevel string

const (
	ConfidenceHigh   ConfidenceLevel = "High"
	ConfidenceMedium ConfidenceLevel = "Medium"
	ConfidenceLow    ConfidenceLevel = "Low"
)

// String returns the string representation.
func (c ConfidenceLevel) String() string {
	return string(c)
}

// ConfidenceThresholds defines the thresholds for confidence level classification.
// These thresholds are based on empirical analysis of dead code detection accuracy:
// - High (>=0.8): Private/unexported symbols with no references have very low false positive rates
// - Medium (>=0.5): Exported symbols without internal usage may still be part of public API
// - Low (<0.5): Symbols matching dynamic usage patterns (reflection, callbacks) need manual review
type ConfidenceThresholds struct {
	HighThreshold   float64 // Confidence >= this is "High" (default: 0.8)
	MediumThreshold float64 // Confidence >= this (and < High) is "Medium" (default: 0.5)
}

// DefaultConfidenceThresholds returns the default confidence thresholds.
func DefaultConfidenceThresholds() ConfidenceThresholds {
	return ConfidenceThresholds{
		HighThreshold:   0.8,
		MediumThreshold: 0.5,
	}
}

// Global confidence thresholds (can be overridden via SetConfidenceThresholds)
var (
	confidenceThresholds   = DefaultConfidenceThresholds()
	confidenceThresholdsMu sync.RWMutex
)

// SetConfidenceThresholds allows customizing the confidence level thresholds.
// This function is thread-safe and typically called once at startup.
func SetConfidenceThresholds(thresholds ConfidenceThresholds) {
	confidenceThresholdsMu.Lock()
	defer confidenceThresholdsMu.Unlock()

	if thresholds.HighThreshold > 0 && thresholds.HighThreshold <= 1 {
		confidenceThresholds.HighThreshold = thresholds.HighThreshold
	}
	if thresholds.MediumThreshold > 0 && thresholds.MediumThreshold <= 1 {
		confidenceThresholds.MediumThreshold = thresholds.MediumThreshold
	}
}

// GetConfidenceThresholds returns the current confidence thresholds.
// This function is thread-safe.
func GetConfidenceThresholds() ConfidenceThresholds {
	confidenceThresholdsMu.RLock()
	defer confidenceThresholdsMu.RUnlock()
	return confidenceThresholds
}

// ItemType classifies the type of dead code item.
type ItemType string

const (
	ItemTypeFunction    ItemType = "function"
	ItemTypeClass       ItemType = "class"
	ItemTypeVariable    ItemType = "variable"
	ItemTypeUnreachable ItemType = "unreachable"
)

// String returns the string representation.
func (i ItemType) String() string {
	return string(i)
}

// Item represents an individual dead code item within a file.
type Item struct {
	Type   ItemType `json:"item_type" toon:"item_type"`
	Name   string   `json:"name" toon:"name"`
	Line   uint32   `json:"line" toon:"line"`
	Reason string   `json:"reason" toon:"reason"`
}

// FileMetrics contains file-level dead code metrics with items.
type FileMetrics struct {
	Path              string          `json:"path" toon:"path"`
	DeadLines         int             `json:"dead_lines" toon:"dead_lines"`
	TotalLines        int             `json:"total_lines" toon:"total_lines"`
	DeadPercentage    float32         `json:"dead_percentage" toon:"dead_percentage"`
	DeadFunctions     int             `json:"dead_functions" toon:"dead_functions"`
	DeadClasses       int             `json:"dead_classes" toon:"dead_classes"`
	DeadModules       int             `json:"dead_modules" toon:"dead_modules"`
	UnreachableBlocks int             `json:"unreachable_blocks" toon:"unreachable_blocks"`
	DeadScore         float32         `json:"dead_score" toon:"dead_score"`
	Confidence        ConfidenceLevel `json:"confidence" toon:"confidence"`
	Items             []Item          `json:"items" toon:"items"`
}

// CalculateScore calculates the dead code score using weighted algorithm.
func (f *FileMetrics) CalculateScore() {
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
func (f *FileMetrics) UpdatePercentage() {
	if f.TotalLines > 0 {
		f.DeadPercentage = float32(f.DeadLines) / float32(f.TotalLines) * 100.0
	}
}

// AddItem adds a dead code item and updates counts.
func (f *FileMetrics) AddItem(item Item) {
	switch item.Type {
	case ItemTypeFunction:
		f.DeadFunctions++
		f.DeadLines += 10 // Estimate 10 lines per function
	case ItemTypeClass:
		f.DeadClasses++
		f.DeadLines += 10 // Estimate 10 lines per class
	case ItemTypeVariable:
		f.DeadModules++  // Variables tracked in modules counter
		f.DeadLines += 1 // Estimate 1 line per variable
	case ItemTypeUnreachable:
		f.UnreachableBlocks++
		f.DeadLines += 3 // Estimate 3 lines per unreachable block
	}
	f.Items = append(f.Items, item)
}

// RankingSummary provides aggregate statistics.
type RankingSummary struct {
	TotalFilesAnalyzed int     `json:"total_files_analyzed" toon:"total_files_analyzed"`
	FilesWithDeadCode  int     `json:"files_with_dead_code" toon:"files_with_dead_code"`
	TotalDeadLines     int     `json:"total_dead_lines" toon:"total_dead_lines"`
	DeadPercentage     float32 `json:"dead_percentage" toon:"dead_percentage"`
	DeadFunctions      int     `json:"dead_functions" toon:"dead_functions"`
	DeadClasses        int     `json:"dead_classes" toon:"dead_classes"`
	DeadModules        int     `json:"dead_modules" toon:"dead_modules"`
	UnreachableBlocks  int     `json:"unreachable_blocks" toon:"unreachable_blocks"`
}

// AnalysisConfig holds configuration for dead code analysis.
type AnalysisConfig struct {
	IncludeUnreachable bool `json:"include_unreachable" toon:"include_unreachable"`
	IncludeTests       bool `json:"include_tests" toon:"include_tests"`
	MinDeadLines       int  `json:"min_dead_lines" toon:"min_dead_lines"`
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

// String returns the string representation.
func (r ReferenceType) String() string {
	return string(r)
}

// Kind classifies the type of dead code detected.
type Kind string

const (
	KindFunction    Kind = "unused_function"
	KindClass       Kind = "unused_class"
	KindVariable    Kind = "unused_variable"
	KindUnreachable Kind = "unreachable_code"
	KindDeadBranch  Kind = "dead_branch"
)

// String returns the string representation.
func (k Kind) String() string {
	return string(k)
}

// ReferenceEdge represents a relationship between two code elements.
type ReferenceEdge struct {
	From       uint32        `json:"from" toon:"from"`
	To         uint32        `json:"to" toon:"to"`
	Type       ReferenceType `json:"type" toon:"type"`
	Confidence float64       `json:"confidence" toon:"confidence"`
}

// ReferenceNode represents a code element in the reference graph.
type ReferenceNode struct {
	ID         uint32 `json:"id" toon:"id"`
	Name       string `json:"name" toon:"name"`
	File       string `json:"file" toon:"file"`
	Line       uint32 `json:"line" toon:"line"`
	EndLine    uint32 `json:"end_line" toon:"end_line"`
	Kind       string `json:"kind" toon:"kind"` // function, class, variable
	Language   string `json:"language" toon:"language"`
	IsExported bool   `json:"is_exported" toon:"is_exported"`
	IsEntry    bool   `json:"is_entry" toon:"is_entry"`
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

// Function represents an unused function detected in the codebase.
type Function struct {
	Name             string          `json:"name" toon:"name"`
	File             string          `json:"file" toon:"file"`
	Line             uint32          `json:"line" toon:"line"`
	EndLine          uint32          `json:"end_line" toon:"end_line"`
	Visibility       string          `json:"visibility" toon:"visibility"` // public, private, internal
	Confidence       float64         `json:"confidence" toon:"confidence"` // 0.0-1.0, how certain we are it's dead
	ConfidenceLevel  ConfidenceLevel `json:"confidence_level" toon:"confidence_level"`
	ConfidenceReason string          `json:"confidence_reason" toon:"confidence_reason"` // Why we have this confidence level
	Reason           string          `json:"reason" toon:"reason"`                       // Why it's considered dead
	Kind             Kind            `json:"kind,omitempty" toon:"kind,omitempty"`
	NodeID           uint32          `json:"node_id,omitempty" toon:"node_id,omitempty"`
}

// SetConfidenceLevel sets the confidence level and reason based on the numeric confidence.
// Uses configurable thresholds from GetConfidenceThresholds().
func (f *Function) SetConfidenceLevel() {
	thresholds := GetConfidenceThresholds()
	if f.Confidence >= thresholds.HighThreshold {
		f.ConfidenceLevel = ConfidenceHigh
		f.ConfidenceReason = "High confidence: private/unexported, no references in call graph"
	} else if f.Confidence >= thresholds.MediumThreshold {
		f.ConfidenceLevel = ConfidenceMedium
		f.ConfidenceReason = "Medium confidence: exported but no internal references found"
	} else {
		f.ConfidenceLevel = ConfidenceLow
		f.ConfidenceReason = "Low confidence: matches patterns that may have dynamic usage"
	}
}

// Class represents an unused class/struct/type.
type Class struct {
	Name             string          `json:"name" toon:"name"`
	File             string          `json:"file" toon:"file"`
	Line             uint32          `json:"line" toon:"line"`
	EndLine          uint32          `json:"end_line" toon:"end_line"`
	Visibility       string          `json:"visibility" toon:"visibility"`
	Confidence       float64         `json:"confidence" toon:"confidence"`
	ConfidenceLevel  ConfidenceLevel `json:"confidence_level" toon:"confidence_level"`
	ConfidenceReason string          `json:"confidence_reason" toon:"confidence_reason"`
	Reason           string          `json:"reason" toon:"reason"`
	Kind             Kind            `json:"kind,omitempty" toon:"kind,omitempty"`
	NodeID           uint32          `json:"node_id,omitempty" toon:"node_id,omitempty"`
}

// SetConfidenceLevel sets the confidence level and reason based on the numeric confidence.
// Uses configurable thresholds from GetConfidenceThresholds().
func (c *Class) SetConfidenceLevel() {
	thresholds := GetConfidenceThresholds()
	if c.Confidence >= thresholds.HighThreshold {
		c.ConfidenceLevel = ConfidenceHigh
		c.ConfidenceReason = "High confidence: private/unexported type, no references in codebase"
	} else if c.Confidence >= thresholds.MediumThreshold {
		c.ConfidenceLevel = ConfidenceMedium
		c.ConfidenceReason = "Medium confidence: exported type but no internal usages found"
	} else {
		c.ConfidenceLevel = ConfidenceLow
		c.ConfidenceReason = "Low confidence: may be used via reflection or as public API"
	}
}

// Variable represents an unused variable.
type Variable struct {
	Name             string          `json:"name" toon:"name"`
	File             string          `json:"file" toon:"file"`
	Line             uint32          `json:"line" toon:"line"`
	Visibility       string          `json:"visibility" toon:"visibility"`
	Confidence       float64         `json:"confidence" toon:"confidence"`
	ConfidenceLevel  ConfidenceLevel `json:"confidence_level" toon:"confidence_level"`
	ConfidenceReason string          `json:"confidence_reason" toon:"confidence_reason"`
	Reason           string          `json:"reason,omitempty" toon:"reason,omitempty"`
	Kind             Kind            `json:"kind,omitempty" toon:"kind,omitempty"`
	NodeID           uint32          `json:"node_id,omitempty" toon:"node_id,omitempty"`
}

// SetConfidenceLevel sets the confidence level and reason based on the numeric confidence.
// Uses configurable thresholds from GetConfidenceThresholds().
func (v *Variable) SetConfidenceLevel() {
	thresholds := GetConfidenceThresholds()
	if v.Confidence >= thresholds.HighThreshold {
		v.ConfidenceLevel = ConfidenceHigh
		v.ConfidenceReason = "High confidence: private/unexported variable, no references found"
	} else if v.Confidence >= thresholds.MediumThreshold {
		v.ConfidenceLevel = ConfidenceMedium
		v.ConfidenceReason = "Medium confidence: exported variable but no internal usages found"
	} else {
		v.ConfidenceLevel = ConfidenceLow
		v.ConfidenceReason = "Low confidence: may be accessed dynamically or via reflection"
	}
}

// UnreachableBlock represents code that can never execute.
type UnreachableBlock struct {
	File      string `json:"file" toon:"file"`
	StartLine uint32 `json:"start_line" toon:"start_line"`
	EndLine   uint32 `json:"end_line" toon:"end_line"`
	Reason    string `json:"reason" toon:"reason"` // e.g., "after return", "dead branch"
}

// Summary provides aggregate statistics.
type Summary struct {
	TotalDeadFunctions     int            `json:"total_dead_functions" toon:"total_dead_functions"`
	TotalDeadVariables     int            `json:"total_dead_variables" toon:"total_dead_variables"`
	TotalDeadClasses       int            `json:"total_dead_classes" toon:"total_dead_classes"`
	TotalUnreachableBlocks int            `json:"total_unreachable_blocks" toon:"total_unreachable_blocks"`
	TotalUnreachableLines  int            `json:"total_unreachable_lines" toon:"total_unreachable_lines"`
	DeadCodePercentage     float64        `json:"dead_code_percentage" toon:"dead_code_percentage"`
	ByFile                 map[string]int `json:"by_file" toon:"by_file"`
	ByKind                 map[Kind]int   `json:"by_kind,omitempty" toon:"-"`
	TotalFilesAnalyzed     int            `json:"total_files_analyzed" toon:"total_files_analyzed"`
	TotalLinesAnalyzed     int            `json:"total_lines_analyzed" toon:"total_lines_analyzed"`
	TotalNodesInGraph      int            `json:"total_nodes_in_graph,omitempty" toon:"total_nodes_in_graph,omitempty"`
	ReachableNodes         int            `json:"reachable_nodes,omitempty" toon:"reachable_nodes,omitempty"`
	UnreachableNodes       int            `json:"unreachable_nodes,omitempty" toon:"unreachable_nodes,omitempty"`
	ConfidenceLevel        float64        `json:"confidence_level,omitempty" toon:"confidence_level,omitempty"`
}

// NewSummary creates an initialized summary.
func NewSummary() Summary {
	return Summary{
		ByFile: make(map[string]int),
		ByKind: make(map[Kind]int),
	}
}

// AddFunction updates the summary with a dead function.
func (s *Summary) AddFunction(f Function) {
	s.TotalDeadFunctions++
	s.ByFile[f.File]++
	if f.Kind != "" {
		s.ByKind[f.Kind]++
	} else {
		s.ByKind[KindFunction]++
	}
}

// AddVariable updates the summary with a dead variable.
func (s *Summary) AddVariable(v Variable) {
	s.TotalDeadVariables++
	s.ByFile[v.File]++
	if v.Kind != "" {
		s.ByKind[v.Kind]++
	} else {
		s.ByKind[KindVariable]++
	}
}

// AddClass updates the summary with a dead class.
func (s *Summary) AddClass(c Class) {
	s.TotalDeadClasses++
	s.ByFile[c.File]++
	if c.Kind != "" {
		s.ByKind[c.Kind]++
	} else {
		s.ByKind[KindClass]++
	}
}

// AddUnreachableBlock updates the summary with unreachable code.
func (s *Summary) AddUnreachableBlock(b UnreachableBlock) {
	lines := int(b.EndLine - b.StartLine + 1)
	s.TotalUnreachableLines += lines
	s.ByFile[b.File] += lines
	s.ByKind[KindUnreachable] += lines
}

// CalculatePercentage computes dead code percentage.
func (s *Summary) CalculatePercentage() {
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

// Analysis represents the full dead code detection result.
type Analysis struct {
	DeadFunctions   []Function         `json:"dead_functions" toon:"dead_functions"`
	DeadVariables   []Variable         `json:"dead_variables" toon:"dead_variables"`
	DeadClasses     []Class            `json:"dead_classes,omitempty" toon:"dead_classes,omitempty"`
	UnreachableCode []UnreachableBlock `json:"unreachable_code" toon:"unreachable_code"`
	Summary         Summary            `json:"summary" toon:"summary"`
	CallGraph       *CallGraph         `json:"call_graph,omitempty" toon:"-"`
}

// Report is the output format for dead code analysis.
type Report struct {
	Summary           RankingSummary `json:"summary" toon:"summary"`
	Files             []FileMetrics  `json:"files" toon:"files"`
	TotalFiles        int            `json:"total_files" toon:"total_files"`
	AnalyzedFiles     int            `json:"analyzed_files" toon:"analyzed_files"`
	AnalysisTimestamp time.Time      `json:"analysis_timestamp,omitempty" toon:"analysis_timestamp,omitempty"`
	Config            AnalysisConfig `json:"config,omitempty" toon:"config,omitempty"`
}

// NewReport creates a new dead code report.
func NewReport() *Report {
	return &Report{
		Files: make([]FileMetrics, 0),
		Config: AnalysisConfig{
			IncludeUnreachable: false,
			IncludeTests:       false,
			MinDeadLines:       10,
		},
	}
}

// FromAnalysis converts the internal Analysis to report format.
func (r *Report) FromAnalysis(analysis *Analysis) {
	// Group items by file
	fileMap := make(map[string]*FileMetrics)

	// Process dead functions
	for _, df := range analysis.DeadFunctions {
		fm := r.getOrCreateFileMetrics(fileMap, df.File)
		fm.AddItem(Item{
			Type:   ItemTypeFunction,
			Name:   df.Name,
			Line:   df.Line,
			Reason: df.Reason,
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
		fm.AddItem(Item{
			Type:   ItemTypeClass,
			Name:   dc.Name,
			Line:   dc.Line,
			Reason: dc.Reason,
		})
	}

	// Process dead variables
	for _, dv := range analysis.DeadVariables {
		fm := r.getOrCreateFileMetrics(fileMap, dv.File)
		fm.AddItem(Item{
			Type:   ItemTypeVariable,
			Name:   dv.Name,
			Line:   dv.Line,
			Reason: dv.Reason,
		})
	}

	// Process unreachable blocks
	for _, ub := range analysis.UnreachableCode {
		fm := r.getOrCreateFileMetrics(fileMap, ub.File)
		fm.AddItem(Item{
			Type:   ItemTypeUnreachable,
			Name:   "unreachable_block",
			Line:   ub.StartLine,
			Reason: ub.Reason,
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

func (r *Report) getOrCreateFileMetrics(fileMap map[string]*FileMetrics, path string) *FileMetrics {
	if fm, exists := fileMap[path]; exists {
		return fm
	}
	fm := &FileMetrics{
		Path:       path,
		Confidence: ConfidenceMedium,
		Items:      make([]Item, 0),
	}
	fileMap[path] = fm
	return fm
}

func (r *Report) sortFilesByScore() {
	// Simple bubble sort for now (files list is typically small)
	for i := 0; i < len(r.Files)-1; i++ {
		for j := 0; j < len(r.Files)-i-1; j++ {
			if r.Files[j].DeadScore < r.Files[j+1].DeadScore {
				r.Files[j], r.Files[j+1] = r.Files[j+1], r.Files[j]
			}
		}
	}
}

func (r *Report) buildSummary(analysis *Analysis) {
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
