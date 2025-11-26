package models

// ReferenceType classifies the relationship between code elements.
type ReferenceType string

const (
	RefDirectCall     ReferenceType = "direct_call"
	RefIndirectCall   ReferenceType = "indirect_call"
	RefImport         ReferenceType = "import"
	RefInheritance    ReferenceType = "inheritance"
	RefTypeReference  ReferenceType = "type_reference"
	RefDynamicDispatch ReferenceType = "dynamic_dispatch"
)

// DeadCodeKind classifies the type of dead code detected.
type DeadCodeKind string

const (
	DeadKindFunction       DeadCodeKind = "unused_function"
	DeadKindClass          DeadCodeKind = "unused_class"
	DeadKindVariable       DeadCodeKind = "unused_variable"
	DeadKindUnreachable    DeadCodeKind = "unreachable_code"
	DeadKindDeadBranch     DeadCodeKind = "dead_branch"
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
	Nodes       map[uint32]*ReferenceNode `json:"nodes"`
	Edges       []ReferenceEdge           `json:"edges"`
	EntryPoints []uint32                  `json:"entry_points"`
	EdgeIndex   map[uint32][]int          `json:"-"` // node -> edge indices (outgoing)
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
	Name       string       `json:"name"`
	File       string       `json:"file"`
	Line       uint32       `json:"line"`
	EndLine    uint32       `json:"end_line"`
	Visibility string       `json:"visibility"` // public, private, internal
	Confidence float64      `json:"confidence"` // 0.0-1.0, how certain we are it's dead
	Reason     string       `json:"reason"`     // Why it's considered dead
	Kind       DeadCodeKind `json:"kind,omitempty"`
	NodeID     uint32       `json:"node_id,omitempty"`
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
	Name       string       `json:"name"`
	File       string       `json:"file"`
	Line       uint32       `json:"line"`
	EndLine    uint32       `json:"end_line"`
	Confidence float64      `json:"confidence"`
	Reason     string       `json:"reason"`
	Kind       DeadCodeKind `json:"kind,omitempty"`
	NodeID     uint32       `json:"node_id,omitempty"`
}

// DeadVariable represents an unused variable.
type DeadVariable struct {
	Name       string       `json:"name"`
	File       string       `json:"file"`
	Line       uint32       `json:"line"`
	Confidence float64      `json:"confidence"`
	Reason     string       `json:"reason,omitempty"`
	Kind       DeadCodeKind `json:"kind,omitempty"`
	NodeID     uint32       `json:"node_id,omitempty"`
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
	TotalDeadFunctions    int                      `json:"total_dead_functions"`
	TotalDeadVariables    int                      `json:"total_dead_variables"`
	TotalDeadClasses      int                      `json:"total_dead_classes"`
	TotalUnreachableLines int                      `json:"total_unreachable_lines"`
	DeadCodePercentage    float64                  `json:"dead_code_percentage"`
	ByFile                map[string]int           `json:"by_file"`
	ByKind                map[DeadCodeKind]int     `json:"by_kind,omitempty"`
	TotalFilesAnalyzed    int                      `json:"total_files_analyzed"`
	TotalLinesAnalyzed    int                      `json:"total_lines_analyzed"`
	TotalNodesInGraph     int                      `json:"total_nodes_in_graph,omitempty"`
	ReachableNodes        int                      `json:"reachable_nodes,omitempty"`
	UnreachableNodes      int                      `json:"unreachable_nodes,omitempty"`
	ConfidenceLevel       float64                  `json:"confidence_level,omitempty"`
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
