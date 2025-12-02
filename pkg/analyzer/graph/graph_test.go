package graph

import (
	"os"
	"path/filepath"
	"testing"
)

func TestNew(t *testing.T) {
	a := New()
	if a == nil {
		t.Fatal("New() returned nil")
	}
	if a.parser == nil {
		t.Error("analyzer.parser is nil")
	}
	if a.scope != ScopeFunction {
		t.Errorf("default scope = %v, want %v", a.scope, ScopeFunction)
	}
	a.Close()
}

func TestNewWithOptions(t *testing.T) {
	a := New(
		WithScope(ScopeFile),
		WithMaxFileSize(1024),
	)

	if a.scope != ScopeFile {
		t.Errorf("scope = %v, want %v", a.scope, ScopeFile)
	}
	if a.maxFileSize != 1024 {
		t.Errorf("maxFileSize = %d, want 1024", a.maxFileSize)
	}
	a.Close()
}

func TestAnalyzeProject_FileScope(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "a.go")
	code1 := `package main

import "fmt"

func main() {
	fmt.Println("hello")
}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	file2 := filepath.Join(tmpDir, "b.go")
	code2 := `package main

func helper() int {
	return 42
}
`
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeFile))
	defer a.Close()

	graph, err := a.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(graph.Nodes) != 2 {
		t.Errorf("len(Nodes) = %d, want 2", len(graph.Nodes))
	}
}

func TestAnalyzeProject_FunctionScope(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "main.go")
	code1 := `package main

func main() {
	helper()
}

func helper() int {
	return 42
}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeFunction))
	defer a.Close()

	graph, err := a.AnalyzeProject([]string{file1})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(graph.Nodes) < 2 {
		t.Errorf("len(Nodes) = %d, want >= 2", len(graph.Nodes))
	}
}

func TestDependencyGraph_AddNode(t *testing.T) {
	g := NewDependencyGraph()

	node := Node{
		ID:   "test:func",
		Name: "func",
		Type: NodeFunction,
		File: "test.go",
	}

	g.AddNode(node)

	if len(g.Nodes) != 1 {
		t.Errorf("len(Nodes) = %d, want 1", len(g.Nodes))
	}
	if g.Nodes[0].ID != "test:func" {
		t.Errorf("Node.ID = %q, want %q", g.Nodes[0].ID, "test:func")
	}
}

func TestDependencyGraph_AddEdge(t *testing.T) {
	g := NewDependencyGraph()

	edge := Edge{
		From: "a",
		To:   "b",
		Type: EdgeCall,
	}

	g.AddEdge(edge)

	if len(g.Edges) != 1 {
		t.Errorf("len(Edges) = %d, want 1", len(g.Edges))
	}
	if g.Edges[0].From != "a" {
		t.Errorf("Edge.From = %q, want %q", g.Edges[0].From, "a")
	}
}

func TestCalculateMetrics_Empty(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	metrics := a.CalculateMetrics(g)

	if metrics.Summary.TotalNodes != 0 {
		t.Errorf("TotalNodes = %d, want 0", metrics.Summary.TotalNodes)
	}
	if metrics.Summary.TotalEdges != 0 {
		t.Errorf("TotalEdges = %d, want 0", metrics.Summary.TotalEdges)
	}
}

func TestCalculateMetrics_Simple(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	g.AddNode(Node{ID: "a", Name: "a", Type: NodeFunction})
	g.AddNode(Node{ID: "b", Name: "b", Type: NodeFunction})
	g.AddNode(Node{ID: "c", Name: "c", Type: NodeFunction})
	g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})
	g.AddEdge(Edge{From: "b", To: "c", Type: EdgeCall})

	metrics := a.CalculateMetrics(g)

	if metrics.Summary.TotalNodes != 3 {
		t.Errorf("TotalNodes = %d, want 3", metrics.Summary.TotalNodes)
	}
	if metrics.Summary.TotalEdges != 2 {
		t.Errorf("TotalEdges = %d, want 2", metrics.Summary.TotalEdges)
	}
	if len(metrics.NodeMetrics) != 3 {
		t.Errorf("len(NodeMetrics) = %d, want 3", len(metrics.NodeMetrics))
	}
}

func TestCalculatePageRankOnly(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	g.AddNode(Node{ID: "a", Name: "a", Type: NodeFunction})
	g.AddNode(Node{ID: "b", Name: "b", Type: NodeFunction})
	g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})

	metrics := a.CalculatePageRankOnly(g)

	if metrics.Summary.TotalNodes != 2 {
		t.Errorf("TotalNodes = %d, want 2", metrics.Summary.TotalNodes)
	}

	// Node b should have higher PageRank (it receives an edge)
	var aRank, bRank float64
	for _, nm := range metrics.NodeMetrics {
		if nm.NodeID == "a" {
			aRank = nm.PageRank
		}
		if nm.NodeID == "b" {
			bRank = nm.PageRank
		}
	}

	if bRank <= aRank {
		t.Errorf("Expected b.PageRank (%f) > a.PageRank (%f)", bRank, aRank)
	}
}

func TestDetectCycles_NoCycles(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	g.AddNode(Node{ID: "a", Name: "a", Type: NodeFunction})
	g.AddNode(Node{ID: "b", Name: "b", Type: NodeFunction})
	g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})

	cycles := a.DetectCycles(g)

	if len(cycles) != 0 {
		t.Errorf("expected no cycles, got %d", len(cycles))
	}
}

func TestDetectCycles_WithCycle(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	g.AddNode(Node{ID: "a", Name: "a", Type: NodeFunction})
	g.AddNode(Node{ID: "b", Name: "b", Type: NodeFunction})
	g.AddNode(Node{ID: "c", Name: "c", Type: NodeFunction})
	g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})
	g.AddEdge(Edge{From: "b", To: "c", Type: EdgeCall})
	g.AddEdge(Edge{From: "c", To: "a", Type: EdgeCall})

	cycles := a.DetectCycles(g)

	if len(cycles) != 1 {
		t.Errorf("expected 1 cycle, got %d", len(cycles))
	}
	if len(cycles) > 0 && len(cycles[0]) != 3 {
		t.Errorf("expected cycle of 3 nodes, got %d", len(cycles[0]))
	}
}

func TestPruneGraph(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	for i := 0; i < 10; i++ {
		id := string(rune('a' + i))
		g.AddNode(Node{ID: id, Name: id, Type: NodeFunction})
	}
	// Create a chain: a -> b -> c -> ... -> j
	for i := 0; i < 9; i++ {
		from := string(rune('a' + i))
		to := string(rune('a' + i + 1))
		g.AddEdge(Edge{From: from, To: to, Type: EdgeCall})
	}

	pruned := a.PruneGraph(g, 5, 10)

	if len(pruned.Nodes) > 5 {
		t.Errorf("len(Nodes) = %d, want <= 5", len(pruned.Nodes))
	}
}

func TestScope_Values(t *testing.T) {
	// Scope is a string type, test constants
	if ScopeFile != "file" {
		t.Errorf("ScopeFile = %q, want %q", ScopeFile, "file")
	}
	if ScopeFunction != "function" {
		t.Errorf("ScopeFunction = %q, want %q", ScopeFunction, "function")
	}
	if ScopeModule != "module" {
		t.Errorf("ScopeModule = %q, want %q", ScopeModule, "module")
	}
}

func TestNodeType_String(t *testing.T) {
	tests := []struct {
		nt   NodeType
		want string
	}{
		{NodeFile, "file"},
		{NodeFunction, "function"},
		{NodeModule, "module"},
	}

	for _, tt := range tests {
		got := tt.nt.String()
		if got != tt.want {
			t.Errorf("NodeType(%v).String() = %q, want %q", tt.nt, got, tt.want)
		}
	}
}

func TestEdgeType_String(t *testing.T) {
	tests := []struct {
		et   EdgeType
		want string
	}{
		{EdgeCall, "call"},
		{EdgeImport, "import"},
		{EdgeReference, "reference"},
	}

	for _, tt := range tests {
		got := tt.et.String()
		if got != tt.want {
			t.Errorf("EdgeType(%v).String() = %q, want %q", tt.et, got, tt.want)
		}
	}
}

func TestMatchesImport(t *testing.T) {
	tests := []struct {
		filePath   string
		importPath string
		want       bool
	}{
		{"/src/utils/helper.go", "utils/helper", true},
		{"/src/main.go", "utils/helper", false},
		{"same/path", "same/path", false}, // Same path should not match
	}

	for _, tt := range tests {
		got := matchesImport(tt.filePath, tt.importPath)
		if got != tt.want {
			t.Errorf("matchesImport(%q, %q) = %v, want %v",
				tt.filePath, tt.importPath, got, tt.want)
		}
	}
}
