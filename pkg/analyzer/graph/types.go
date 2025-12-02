package graph

// Node represents a node in the dependency graph.
type Node struct {
	ID         string            `json:"id" toon:"id"`
	Name       string            `json:"name" toon:"name"`
	Type       NodeType          `json:"type" toon:"type"` // file, function, class, module
	File       string            `json:"file" toon:"file"`
	Line       uint32            `json:"line,omitempty" toon:"line,omitempty"`
	Attributes map[string]string `json:"attributes,omitempty" toon:"attributes,omitempty"`
}

// NodeType represents the type of graph node.
type NodeType string

const (
	NodeFile      NodeType = "file"
	NodeFunction  NodeType = "function"
	NodeClass     NodeType = "class"
	NodeModule    NodeType = "module"
	NodePackage   NodeType = "package"
	NodeTrait     NodeType = "trait"
	NodeInterface NodeType = "interface"
)

// String returns the string representation.
func (n NodeType) String() string {
	return string(n)
}

// Edge represents a dependency between nodes.
type Edge struct {
	From   string   `json:"from" toon:"from"`
	To     string   `json:"to" toon:"to"`
	Type   EdgeType `json:"type" toon:"type"`
	Weight float64  `json:"weight,omitempty" toon:"weight,omitempty"`
}

// EdgeType represents the type of dependency.
type EdgeType string

const (
	EdgeImport    EdgeType = "import"
	EdgeCall      EdgeType = "call"
	EdgeInherit   EdgeType = "inherit"
	EdgeImplement EdgeType = "implement"
	EdgeReference EdgeType = "reference"
	EdgeUses      EdgeType = "uses"
)

// String returns the string representation.
func (e EdgeType) String() string {
	return string(e)
}

// DependencyGraph represents the full graph structure.
type DependencyGraph struct {
	Nodes []Node `json:"nodes" toon:"nodes"`
	Edges []Edge `json:"edges" toon:"edges"`
}

// NewDependencyGraph creates an empty graph.
func NewDependencyGraph() *DependencyGraph {
	return &DependencyGraph{
		Nodes: make([]Node, 0),
		Edges: make([]Edge, 0),
	}
}

// AddNode adds a node to the graph.
func (g *DependencyGraph) AddNode(node Node) {
	g.Nodes = append(g.Nodes, node)
}

// AddEdge adds an edge to the graph.
func (g *DependencyGraph) AddEdge(edge Edge) {
	g.Edges = append(g.Edges, edge)
}

// Metrics represents centrality and other graph metrics.
type Metrics struct {
	NodeMetrics []NodeMetric `json:"node_metrics" toon:"node_metrics"`
	Summary     Summary      `json:"summary" toon:"summary"`
}

// NodeMetric represents computed metrics for a single node.
type NodeMetric struct {
	NodeID                string  `json:"node_id" toon:"node_id"`
	Name                  string  `json:"name" toon:"name"`
	PageRank              float64 `json:"pagerank" toon:"pagerank"`
	BetweennessCentrality float64 `json:"betweenness_centrality" toon:"betweenness_centrality"`
	ClosenessCentrality   float64 `json:"closeness_centrality" toon:"closeness_centrality"`
	EigenvectorCentrality float64 `json:"eigenvector_centrality" toon:"eigenvector_centrality"`
	HarmonicCentrality    float64 `json:"harmonic_centrality" toon:"harmonic_centrality"`
	InDegree              int     `json:"in_degree" toon:"in_degree"`
	OutDegree             int     `json:"out_degree" toon:"out_degree"`
	ClusteringCoef        float64 `json:"clustering_coefficient" toon:"clustering_coefficient"`
	CommunityID           int     `json:"community_id,omitempty" toon:"community_id,omitempty"`
}

// Summary provides aggregate graph statistics.
type Summary struct {
	TotalNodes                  int      `json:"total_nodes" toon:"total_nodes"`
	TotalEdges                  int      `json:"total_edges" toon:"total_edges"`
	AvgDegree                   float64  `json:"avg_degree" toon:"avg_degree"`
	Density                     float64  `json:"density" toon:"density"`
	Components                  int      `json:"components" toon:"components"`
	LargestComponent            int      `json:"largest_component" toon:"largest_component"`
	StronglyConnectedComponents int      `json:"strongly_connected_components" toon:"strongly_connected_components"`
	CycleCount                  int      `json:"cycle_count" toon:"cycle_count"`
	CycleNodes                  []string `json:"cycle_nodes,omitempty" toon:"cycle_nodes,omitempty"`
	IsCyclic                    bool     `json:"is_cyclic" toon:"is_cyclic"`
	Diameter                    int      `json:"diameter,omitempty" toon:"diameter,omitempty"`
	Radius                      int      `json:"radius,omitempty" toon:"radius,omitempty"`
	ClusteringCoefficient       float64  `json:"clustering_coefficient" toon:"clustering_coefficient"`
	Assortativity               float64  `json:"assortativity" toon:"assortativity"`
	Transitivity                float64  `json:"transitivity" toon:"transitivity"`
	Reciprocity                 float64  `json:"reciprocity,omitempty" toon:"reciprocity,omitempty"`
	Modularity                  float64  `json:"modularity,omitempty" toon:"modularity,omitempty"`
	CommunityCount              int      `json:"community_count,omitempty" toon:"community_count,omitempty"`
}

// MermaidOptions configures Mermaid diagram generation.
type MermaidOptions struct {
	MaxNodes       int              `json:"max_nodes" toon:"max_nodes"`
	MaxEdges       int              `json:"max_edges" toon:"max_edges"`
	ShowComplexity bool             `json:"show_complexity" toon:"show_complexity"`
	GroupByModule  bool             `json:"group_by_module" toon:"group_by_module"`
	NodeComplexity map[string]int   `json:"node_complexity,omitempty" toon:"node_complexity,omitempty"`
	Direction      MermaidDirection `json:"direction" toon:"direction"`
}

// MermaidDirection specifies the graph direction.
type MermaidDirection string

const (
	DirectionTD MermaidDirection = "TD" // Top-down
	DirectionLR MermaidDirection = "LR" // Left-right
	DirectionBT MermaidDirection = "BT" // Bottom-top
	DirectionRL MermaidDirection = "RL" // Right-left
)

// DefaultMermaidOptions returns sensible defaults.
func DefaultMermaidOptions() MermaidOptions {
	return MermaidOptions{
		MaxNodes:       50,
		MaxEdges:       150,
		ShowComplexity: false,
		GroupByModule:  false,
		Direction:      DirectionTD,
	}
}

// ToMermaid generates Mermaid diagram syntax from the graph using default options.
func (g *DependencyGraph) ToMermaid() string {
	return g.ToMermaidWithOptions(DefaultMermaidOptions())
}

// ToMermaidWithOptions generates Mermaid diagram syntax with custom options.
func (g *DependencyGraph) ToMermaidWithOptions(opts MermaidOptions) string {
	var result string
	direction := opts.Direction
	if direction == "" {
		direction = DirectionTD
	}
	result = "graph " + string(direction) + "\n"

	nodes := g.Nodes
	edges := g.Edges

	// Apply pruning if needed
	if opts.MaxNodes > 0 && len(nodes) > opts.MaxNodes {
		nodes = nodes[:opts.MaxNodes]
		nodeSet := make(map[string]bool)
		for _, n := range nodes {
			nodeSet[n.ID] = true
		}
		var filteredEdges []Edge
		for _, e := range edges {
			if nodeSet[e.From] && nodeSet[e.To] {
				filteredEdges = append(filteredEdges, e)
			}
		}
		edges = filteredEdges
	}
	if opts.MaxEdges > 0 && len(edges) > opts.MaxEdges {
		edges = edges[:opts.MaxEdges]
	}

	// Add nodes with optional complexity styling
	for _, node := range nodes {
		label := EscapeMermaidLabel(node.Name)
		if label == "" {
			label = EscapeMermaidLabel(node.ID)
		}
		id := SanitizeMermaidID(node.ID)

		if opts.ShowComplexity && opts.NodeComplexity != nil {
			if complexity, ok := opts.NodeComplexity[node.ID]; ok {
				color := complexityColor(complexity)
				result += "    " + id + "[\"" + label + "\"]:::" + complexityClass(complexity) + "\n"
				result += "    style " + id + " fill:" + color + "\n"
			} else {
				result += "    " + id + "[\"" + label + "\"]\n"
			}
		} else {
			result += "    " + id + "[\"" + label + "\"]\n"
		}
	}

	// Add edges with type-specific arrows
	for _, edge := range edges {
		from := SanitizeMermaidID(edge.From)
		to := SanitizeMermaidID(edge.To)
		arrow := edgeArrow(edge.Type)
		result += "    " + from + " " + arrow + " " + to + "\n"
	}

	return result
}

// edgeArrow returns the Mermaid arrow notation for an edge type.
func edgeArrow(t EdgeType) string {
	switch t {
	case EdgeCall:
		return "-->|calls|"
	case EdgeImport:
		return "-.->|imports|"
	case EdgeInherit:
		return "-->|inherits|"
	case EdgeImplement:
		return "-->|implements|"
	case EdgeUses:
		return "---"
	default:
		return "-->"
	}
}

// complexityColor returns a color based on complexity level.
func complexityColor(complexity int) string {
	switch {
	case complexity <= 3:
		return "#90EE90" // Light green
	case complexity <= 7:
		return "#FFD700" // Gold
	case complexity <= 12:
		return "#FFA500" // Orange
	default:
		return "#FF6347" // Tomato red
	}
}

// complexityClass returns a CSS class name for complexity level.
func complexityClass(complexity int) string {
	switch {
	case complexity <= 3:
		return "low"
	case complexity <= 7:
		return "medium"
	case complexity <= 12:
		return "high"
	default:
		return "critical"
	}
}

// SanitizeMermaidID makes an ID safe for Mermaid diagrams.
func SanitizeMermaidID(id string) string {
	if id == "" {
		return "empty"
	}
	var result []byte
	for i := 0; i < len(id); i++ {
		c := id[i]
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') {
			result = append(result, c)
		} else {
			result = append(result, '_')
		}
	}
	// Ensure ID doesn't start with a number
	if len(result) > 0 && result[0] >= '0' && result[0] <= '9' {
		result = append([]byte{'n'}, result...)
	}
	return string(result)
}

// EscapeMermaidLabel escapes special characters in labels for Mermaid.
func EscapeMermaidLabel(s string) string {
	var result []byte
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch c {
		case '&':
			result = append(result, []byte("&amp;")...)
		case '"':
			result = append(result, []byte("&quot;")...)
		case '<':
			result = append(result, []byte("&lt;")...)
		case '>':
			result = append(result, []byte("&gt;")...)
		case '|':
			result = append(result, []byte("&#124;")...)
		case '[':
			result = append(result, []byte("&#91;")...)
		case ']':
			result = append(result, []byte("&#93;")...)
		case '{':
			result = append(result, []byte("&#123;")...)
		case '}':
			result = append(result, []byte("&#125;")...)
		case '\n':
			result = append(result, []byte("<br/>")...)
		default:
			result = append(result, c)
		}
	}
	return string(result)
}

// Scope determines the granularity of graph nodes.
type Scope string

const (
	ScopeFile     Scope = "file"
	ScopeFunction Scope = "function"
	ScopeModule   Scope = "module"
	ScopePackage  Scope = "package"
)
