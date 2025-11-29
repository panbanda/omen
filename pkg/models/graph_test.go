package models

import (
	"strings"
	"testing"
)

func TestNewDependencyGraph(t *testing.T) {
	g := NewDependencyGraph()

	if g == nil {
		t.Fatal("NewDependencyGraph() returned nil")
	}

	if g.Nodes == nil {
		t.Error("Nodes should be initialized")
	}
	if g.Edges == nil {
		t.Error("Edges should be initialized")
	}

	if len(g.Nodes) != 0 {
		t.Errorf("Nodes should be empty, got %d", len(g.Nodes))
	}
	if len(g.Edges) != 0 {
		t.Errorf("Edges should be empty, got %d", len(g.Edges))
	}
}

func TestDependencyGraph_AddNode(t *testing.T) {
	g := NewDependencyGraph()

	node1 := GraphNode{ID: "node1", Name: "Node 1", Type: NodeFile}
	node2 := GraphNode{ID: "node2", Name: "Node 2", Type: NodeFunction}

	g.AddNode(node1)
	if len(g.Nodes) != 1 {
		t.Errorf("After adding 1 node, got %d nodes", len(g.Nodes))
	}

	g.AddNode(node2)
	if len(g.Nodes) != 2 {
		t.Errorf("After adding 2 nodes, got %d nodes", len(g.Nodes))
	}

	if g.Nodes[0].ID != "node1" {
		t.Errorf("First node ID = %s, expected node1", g.Nodes[0].ID)
	}
	if g.Nodes[1].ID != "node2" {
		t.Errorf("Second node ID = %s, expected node2", g.Nodes[1].ID)
	}
}

func TestDependencyGraph_AddEdge(t *testing.T) {
	g := NewDependencyGraph()

	edge1 := GraphEdge{From: "node1", To: "node2", Type: EdgeImport}
	edge2 := GraphEdge{From: "node2", To: "node3", Type: EdgeCall}

	g.AddEdge(edge1)
	if len(g.Edges) != 1 {
		t.Errorf("After adding 1 edge, got %d edges", len(g.Edges))
	}

	g.AddEdge(edge2)
	if len(g.Edges) != 2 {
		t.Errorf("After adding 2 edges, got %d edges", len(g.Edges))
	}
}

func TestDependencyGraph_ToMermaid(t *testing.T) {
	tests := []struct {
		name          string
		setupGraph    func(*DependencyGraph)
		expectedParts []string
	}{
		{
			name: "empty graph",
			setupGraph: func(g *DependencyGraph) {
			},
			expectedParts: []string{"graph TD"},
		},
		{
			name: "simple nodes",
			setupGraph: func(g *DependencyGraph) {
				g.AddNode(GraphNode{ID: "node1", Name: "Node 1"})
				g.AddNode(GraphNode{ID: "node2", Name: "Node 2"})
			},
			expectedParts: []string{
				"graph TD",
				`node1["Node 1"]`,
				`node2["Node 2"]`,
			},
		},
		{
			name: "node without name uses ID",
			setupGraph: func(g *DependencyGraph) {
				g.AddNode(GraphNode{ID: "node1", Name: ""})
			},
			expectedParts: []string{
				"graph TD",
				`node1["node1"]`,
			},
		},
		{
			name: "import edge",
			setupGraph: func(g *DependencyGraph) {
				g.AddNode(GraphNode{ID: "node1", Name: "A"})
				g.AddNode(GraphNode{ID: "node2", Name: "B"})
				g.AddEdge(GraphEdge{From: "node1", To: "node2", Type: EdgeImport})
			},
			expectedParts: []string{
				"node1 -.->|imports| node2",
			},
		},
		{
			name: "call edge",
			setupGraph: func(g *DependencyGraph) {
				g.AddNode(GraphNode{ID: "node1", Name: "A"})
				g.AddNode(GraphNode{ID: "node2", Name: "B"})
				g.AddEdge(GraphEdge{From: "node1", To: "node2", Type: EdgeCall})
			},
			expectedParts: []string{
				"node1 -->|calls| node2",
			},
		},
		{
			name: "inherit edge",
			setupGraph: func(g *DependencyGraph) {
				g.AddNode(GraphNode{ID: "node1", Name: "A"})
				g.AddNode(GraphNode{ID: "node2", Name: "B"})
				g.AddEdge(GraphEdge{From: "node1", To: "node2", Type: EdgeInherit})
			},
			expectedParts: []string{
				"node1 -->|inherits| node2",
			},
		},
		{
			name: "complex graph",
			setupGraph: func(g *DependencyGraph) {
				g.AddNode(GraphNode{ID: "main.go", Name: "main.go"})
				g.AddNode(GraphNode{ID: "util.go", Name: "util.go"})
				g.AddNode(GraphNode{ID: "helper.go", Name: "helper.go"})
				g.AddEdge(GraphEdge{From: "main.go", To: "util.go", Type: EdgeImport})
				g.AddEdge(GraphEdge{From: "main.go", To: "helper.go", Type: EdgeCall})
			},
			expectedParts: []string{
				"graph TD",
				`main_go["main.go"]`,
				`util_go["util.go"]`,
				`helper_go["helper.go"]`,
				"main_go -.->|imports| util_go",
				"main_go -->|calls| helper_go",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			g := NewDependencyGraph()
			tt.setupGraph(g)

			mermaid := g.ToMermaid()

			for _, expected := range tt.expectedParts {
				if !strings.Contains(mermaid, expected) {
					t.Errorf("Mermaid output missing expected part: %s\nFull output:\n%s", expected, mermaid)
				}
			}
		})
	}
}

func TestSanitizeMermaidID(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "simple alphanumeric",
			input:    "node123",
			expected: "node123",
		},
		{
			name:     "with dots",
			input:    "main.go",
			expected: "main_go",
		},
		{
			name:     "with slashes",
			input:    "pkg/models/graph.go",
			expected: "pkg_models_graph_go",
		},
		{
			name:     "with hyphens",
			input:    "my-component",
			expected: "my_component",
		},
		{
			name:     "with spaces",
			input:    "my component",
			expected: "my_component",
		},
		{
			name:     "special characters",
			input:    "node@#$%^&*()",
			expected: "node_________",
		},
		{
			name:     "mixed case preserved",
			input:    "MyNode123",
			expected: "MyNode123",
		},
		{
			name:     "underscores preserved",
			input:    "my_node_123",
			expected: "my_node_123",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := SanitizeMermaidID(tt.input)
			if got != tt.expected {
				t.Errorf("SanitizeMermaidID(%q) = %q, expected %q", tt.input, got, tt.expected)
			}
		})
	}
}

func TestNodeType_Constants(t *testing.T) {
	types := []NodeType{
		NodeFile,
		NodeFunction,
		NodeClass,
		NodeModule,
		NodePackage,
	}

	for _, nt := range types {
		if string(nt) == "" {
			t.Errorf("NodeType should not be empty: %v", nt)
		}
	}
}

func TestEdgeType_Constants(t *testing.T) {
	types := []EdgeType{
		EdgeImport,
		EdgeCall,
		EdgeInherit,
		EdgeImplement,
		EdgeReference,
	}

	for _, et := range types {
		if string(et) == "" {
			t.Errorf("EdgeType should not be empty: %v", et)
		}
	}
}

func TestDependencyGraph_ComplexScenario(t *testing.T) {
	g := NewDependencyGraph()

	g.AddNode(GraphNode{ID: "fileA", Name: "File A", Type: NodeFile})
	g.AddNode(GraphNode{ID: "fileB", Name: "File B", Type: NodeFile})
	g.AddNode(GraphNode{ID: "funcX", Name: "Function X", Type: NodeFunction, File: "fileA"})
	g.AddNode(GraphNode{ID: "funcY", Name: "Function Y", Type: NodeFunction, File: "fileB"})

	g.AddEdge(GraphEdge{From: "fileA", To: "fileB", Type: EdgeImport})
	g.AddEdge(GraphEdge{From: "funcX", To: "funcY", Type: EdgeCall})

	if len(g.Nodes) != 4 {
		t.Errorf("Expected 4 nodes, got %d", len(g.Nodes))
	}
	if len(g.Edges) != 2 {
		t.Errorf("Expected 2 edges, got %d", len(g.Edges))
	}

	mermaid := g.ToMermaid()
	if !strings.HasPrefix(mermaid, "graph TD") {
		t.Error("Mermaid output should start with 'graph TD'")
	}
}

func TestToMermaidWithOptions(t *testing.T) {
	t.Run("custom direction", func(t *testing.T) {
		g := NewDependencyGraph()
		g.AddNode(GraphNode{ID: "a", Name: "A"})

		opts := MermaidOptions{Direction: DirectionLR}
		mermaid := g.ToMermaidWithOptions(opts)

		if !strings.HasPrefix(mermaid, "graph LR") {
			t.Errorf("Expected 'graph LR', got %s", mermaid)
		}
	})

	t.Run("max nodes pruning", func(t *testing.T) {
		g := NewDependencyGraph()
		g.AddNode(GraphNode{ID: "a", Name: "A"})
		g.AddNode(GraphNode{ID: "b", Name: "B"})
		g.AddNode(GraphNode{ID: "c", Name: "C"})
		g.AddEdge(GraphEdge{From: "a", To: "b", Type: EdgeCall})
		g.AddEdge(GraphEdge{From: "b", To: "c", Type: EdgeCall})

		opts := MermaidOptions{MaxNodes: 2}
		mermaid := g.ToMermaidWithOptions(opts)

		// Should only contain 2 nodes
		if strings.Contains(mermaid, `c["C"]`) {
			t.Error("Should not contain third node when MaxNodes=2")
		}
	})

	t.Run("max edges pruning", func(t *testing.T) {
		g := NewDependencyGraph()
		g.AddNode(GraphNode{ID: "a", Name: "A"})
		g.AddNode(GraphNode{ID: "b", Name: "B"})
		g.AddEdge(GraphEdge{From: "a", To: "b", Type: EdgeCall})
		g.AddEdge(GraphEdge{From: "b", To: "a", Type: EdgeCall})

		opts := MermaidOptions{MaxEdges: 1}
		mermaid := g.ToMermaidWithOptions(opts)

		// Count edge occurrences
		count := strings.Count(mermaid, "-->|calls|")
		if count > 1 {
			t.Errorf("Expected at most 1 edge, got %d", count)
		}
	})

	t.Run("show complexity styling", func(t *testing.T) {
		g := NewDependencyGraph()
		g.AddNode(GraphNode{ID: "simple", Name: "Simple"})
		g.AddNode(GraphNode{ID: "complex", Name: "Complex"})

		opts := MermaidOptions{
			ShowComplexity: true,
			NodeComplexity: map[string]int{
				"simple":  2,
				"complex": 15,
			},
		}
		mermaid := g.ToMermaidWithOptions(opts)

		if !strings.Contains(mermaid, ":::low") {
			t.Error("Should contain low complexity class")
		}
		if !strings.Contains(mermaid, ":::critical") {
			t.Error("Should contain critical complexity class")
		}
		if !strings.Contains(mermaid, "fill:#90EE90") {
			t.Error("Should contain green color for low complexity")
		}
		if !strings.Contains(mermaid, "fill:#FF6347") {
			t.Error("Should contain red color for high complexity")
		}
	})

	t.Run("empty direction uses default", func(t *testing.T) {
		g := NewDependencyGraph()
		g.AddNode(GraphNode{ID: "a", Name: "A"})

		opts := MermaidOptions{Direction: ""}
		mermaid := g.ToMermaidWithOptions(opts)

		if !strings.HasPrefix(mermaid, "graph TD") {
			t.Errorf("Expected default 'graph TD', got %s", mermaid)
		}
	})
}

func TestEdgeArrow(t *testing.T) {
	tests := []struct {
		edgeType EdgeType
		expected string
	}{
		{EdgeCall, "-->|calls|"},
		{EdgeImport, "-.->|imports|"},
		{EdgeInherit, "-->|inherits|"},
		{EdgeImplement, "-->|implements|"},
		{EdgeUses, "---"},
		{EdgeType("unknown"), "-->"},
	}

	for _, tt := range tests {
		t.Run(string(tt.edgeType), func(t *testing.T) {
			got := edgeArrow(tt.edgeType)
			if got != tt.expected {
				t.Errorf("edgeArrow(%v) = %v, want %v", tt.edgeType, got, tt.expected)
			}
		})
	}
}

func TestComplexityColor(t *testing.T) {
	tests := []struct {
		complexity int
		expected   string
	}{
		{1, "#90EE90"},  // Light green
		{3, "#90EE90"},  // Light green
		{4, "#FFD700"},  // Gold
		{7, "#FFD700"},  // Gold
		{8, "#FFA500"},  // Orange
		{12, "#FFA500"}, // Orange
		{13, "#FF6347"}, // Tomato red
		{50, "#FF6347"}, // Tomato red
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := complexityColor(tt.complexity)
			if got != tt.expected {
				t.Errorf("complexityColor(%d) = %v, want %v", tt.complexity, got, tt.expected)
			}
		})
	}
}

func TestComplexityClass(t *testing.T) {
	tests := []struct {
		complexity int
		expected   string
	}{
		{1, "low"},
		{3, "low"},
		{4, "medium"},
		{7, "medium"},
		{8, "high"},
		{12, "high"},
		{13, "critical"},
		{50, "critical"},
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			got := complexityClass(tt.complexity)
			if got != tt.expected {
				t.Errorf("complexityClass(%d) = %v, want %v", tt.complexity, got, tt.expected)
			}
		})
	}
}

func TestEscapeMermaidLabel(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{"plain text", "hello", "hello"},
		{"ampersand", "A & B", "A &amp; B"},
		{"quotes", `say "hi"`, "say &quot;hi&quot;"},
		{"less than", "a < b", "a &lt; b"},
		{"greater than", "a > b", "a &gt; b"},
		{"mixed", `"<>&`, "&quot;&lt;&gt;&amp;"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := EscapeMermaidLabel(tt.input)
			if got != tt.expected {
				t.Errorf("EscapeMermaidLabel(%q) = %q, want %q", tt.input, got, tt.expected)
			}
		})
	}
}

func TestSanitizeMermaidID_StartsWithNumber(t *testing.T) {
	result := SanitizeMermaidID("123abc")
	if result[0] == '1' {
		t.Errorf("SanitizeMermaidID should prefix IDs starting with numbers, got %q", result)
	}
	if result != "n123abc" {
		t.Errorf("SanitizeMermaidID(\"123abc\") = %q, want \"n123abc\"", result)
	}
}

func TestSanitizeMermaidID_Empty(t *testing.T) {
	result := SanitizeMermaidID("")
	if result != "empty" {
		t.Errorf("SanitizeMermaidID(\"\") = %q, want \"empty\"", result)
	}
}
