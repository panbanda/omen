package models

// GraphNode represents a node in the dependency graph.
type GraphNode struct {
	ID         string            `json:"id"`
	Name       string            `json:"name"`
	Type       NodeType          `json:"type"` // file, function, class, module
	File       string            `json:"file"`
	Line       uint32            `json:"line,omitempty"`
	Attributes map[string]string `json:"attributes,omitempty"`
}

// NodeType represents the type of graph node.
type NodeType string

const (
	NodeFile     NodeType = "file"
	NodeFunction NodeType = "function"
	NodeClass    NodeType = "class"
	NodeModule   NodeType = "module"
	NodePackage  NodeType = "package"
)

// GraphEdge represents a dependency between nodes.
type GraphEdge struct {
	From   string   `json:"from"`
	To     string   `json:"to"`
	Type   EdgeType `json:"type"`
	Weight float64  `json:"weight,omitempty"`
}

// EdgeType represents the type of dependency.
type EdgeType string

const (
	EdgeImport    EdgeType = "import"
	EdgeCall      EdgeType = "call"
	EdgeInherit   EdgeType = "inherit"
	EdgeImplement EdgeType = "implement"
	EdgeReference EdgeType = "reference"
)

// DependencyGraph represents the full graph structure.
type DependencyGraph struct {
	Nodes []GraphNode `json:"nodes"`
	Edges []GraphEdge `json:"edges"`
}

// GraphMetrics represents centrality and other graph metrics.
type GraphMetrics struct {
	NodeMetrics []NodeMetric `json:"node_metrics"`
	Summary     GraphSummary `json:"summary"`
}

// NodeMetric represents computed metrics for a single node.
type NodeMetric struct {
	NodeID                string  `json:"node_id"`
	Name                  string  `json:"name"`
	PageRank              float64 `json:"pagerank"`
	BetweennessCentrality float64 `json:"betweenness_centrality"`
	InDegree              int     `json:"in_degree"`
	OutDegree             int     `json:"out_degree"`
	ClusteringCoef        float64 `json:"clustering_coefficient"`
}

// GraphSummary provides aggregate graph statistics.
type GraphSummary struct {
	TotalNodes       int     `json:"total_nodes"`
	TotalEdges       int     `json:"total_edges"`
	AvgDegree        float64 `json:"avg_degree"`
	Density          float64 `json:"density"`
	Components       int     `json:"components"`
	LargestComponent int     `json:"largest_component"`
}

// MermaidGraph generates Mermaid diagram syntax from the graph.
func (g *DependencyGraph) ToMermaid() string {
	var result string
	result = "graph TD\n"

	// Add nodes
	for _, node := range g.Nodes {
		label := node.Name
		if label == "" {
			label = node.ID
		}
		result += "    " + sanitizeMermaidID(node.ID) + "[\"" + label + "\"]\n"
	}

	// Add edges
	for _, edge := range g.Edges {
		arrow := "-->"
		if edge.Type == EdgeInherit {
			arrow = "-.->|inherits|"
		} else if edge.Type == EdgeCall {
			arrow = "-->|calls|"
		}
		result += "    " + sanitizeMermaidID(edge.From) + " " + arrow + " " + sanitizeMermaidID(edge.To) + "\n"
	}

	return result
}

// sanitizeMermaidID makes an ID safe for Mermaid.
func sanitizeMermaidID(id string) string {
	// Replace problematic characters
	result := ""
	for _, c := range id {
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' {
			result += string(c)
		} else {
			result += "_"
		}
	}
	return result
}

// NewDependencyGraph creates an empty graph.
func NewDependencyGraph() *DependencyGraph {
	return &DependencyGraph{
		Nodes: make([]GraphNode, 0),
		Edges: make([]GraphEdge, 0),
	}
}

// AddNode adds a node to the graph.
func (g *DependencyGraph) AddNode(node GraphNode) {
	g.Nodes = append(g.Nodes, node)
}

// AddEdge adds an edge to the graph.
func (g *DependencyGraph) AddEdge(edge GraphEdge) {
	g.Edges = append(g.Edges, edge)
}
