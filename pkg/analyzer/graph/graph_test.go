package graph

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/source"
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

func TestAnalyze_FileScope(t *testing.T) {
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

	graph, err := a.Analyze(context.Background(), []string{file1, file2}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(graph.Nodes) != 2 {
		t.Errorf("len(Nodes) = %d, want 2", len(graph.Nodes))
	}
}

func TestAnalyze_FunctionScope(t *testing.T) {
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

	graph, err := a.Analyze(context.Background(), []string{file1}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
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

func TestToMermaid(t *testing.T) {
	g := NewDependencyGraph()
	g.AddNode(Node{ID: "main", Name: "main", Type: NodeFunction})
	g.AddNode(Node{ID: "helper", Name: "helper", Type: NodeFunction})
	g.AddEdge(Edge{From: "main", To: "helper", Type: EdgeCall})

	mermaid := g.ToMermaid()

	if mermaid == "" {
		t.Error("ToMermaid returned empty string")
	}
	if len(mermaid) < 20 {
		t.Error("ToMermaid output too short")
	}
	// Check for graph direction
	if !contains(mermaid, "graph TD") {
		t.Error("Expected default direction TD")
	}
}

func TestToMermaidWithOptions(t *testing.T) {
	g := NewDependencyGraph()
	g.AddNode(Node{ID: "a", Name: "a", Type: NodeFunction})
	g.AddNode(Node{ID: "b", Name: "b", Type: NodeFunction})
	g.AddNode(Node{ID: "c", Name: "c", Type: NodeFunction})
	g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})
	g.AddEdge(Edge{From: "b", To: "c", Type: EdgeImport})

	opts := MermaidOptions{
		MaxNodes:       2,
		MaxEdges:       5,
		ShowComplexity: true,
		NodeComplexity: map[string]int{"a": 5, "b": 15},
		Direction:      DirectionLR,
	}

	mermaid := g.ToMermaidWithOptions(opts)

	if !contains(mermaid, "graph LR") {
		t.Error("Expected direction LR")
	}
}

func TestToMermaidWithOptions_Pruning(t *testing.T) {
	g := NewDependencyGraph()
	for i := 0; i < 100; i++ {
		id := string(rune('a' + (i % 26)))
		g.AddNode(Node{ID: id, Name: id, Type: NodeFunction})
	}
	for i := 0; i < 50; i++ {
		g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})
	}

	opts := MermaidOptions{
		MaxNodes:  10,
		MaxEdges:  5,
		Direction: DirectionTD,
	}

	mermaid := g.ToMermaidWithOptions(opts)

	if mermaid == "" {
		t.Error("Expected non-empty mermaid output")
	}
}

func TestDefaultMermaidOptions(t *testing.T) {
	opts := DefaultMermaidOptions()

	if opts.MaxNodes != 50 {
		t.Errorf("MaxNodes = %d, want 50", opts.MaxNodes)
	}
	if opts.MaxEdges != 150 {
		t.Errorf("MaxEdges = %d, want 150", opts.MaxEdges)
	}
	if opts.Direction != DirectionTD {
		t.Errorf("Direction = %v, want %v", opts.Direction, DirectionTD)
	}
}

func TestSanitizeMermaidID(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"simple", "simple"},
		{"with space", "with_space"},
		{"with/slash", "with_slash"},
		{"with.dot", "with_dot"},
		{"123starts", "n123starts"},
		{"", "empty"},
		{"CamelCase", "CamelCase"},
	}

	for _, tt := range tests {
		got := SanitizeMermaidID(tt.input)
		if got != tt.want {
			t.Errorf("SanitizeMermaidID(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func TestEscapeMermaidLabel(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"simple", "simple"},
		{"a & b", "a &amp; b"},
		{"<tag>", "&lt;tag&gt;"},
		{`"quoted"`, "&quot;quoted&quot;"},
		{"a|b", "a&#124;b"},
		{"[brackets]", "&#91;brackets&#93;"},
		{"{braces}", "&#123;braces&#125;"},
		{"line\nbreak", "line<br/>break"},
	}

	for _, tt := range tests {
		got := EscapeMermaidLabel(tt.input)
		if got != tt.want {
			t.Errorf("EscapeMermaidLabel(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func TestAnalyze_Ruby(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.rb")

	code := `class Animal
  def speak
    puts "..."
  end
end

class Dog < Animal
  def speak
    puts "Woof!"
  end
end

Dog.new.speak
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeModule))
	defer a.Close()

	graph, err := a.Analyze(context.Background(), []string{path}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if graph == nil {
		t.Fatal("Expected non-nil graph")
	}
}

func TestAnalyze_Python(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.py")

	code := `class Animal:
    def speak(self):
        pass

class Dog(Animal):
    def speak(self):
        print("Woof!")

d = Dog()
d.speak()
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeModule))
	defer a.Close()

	graph, err := a.Analyze(context.Background(), []string{path}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if graph == nil {
		t.Fatal("Expected non-nil graph")
	}
}

func TestAnalyze_TypeScript(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.ts")

	code := `import { helper } from './utils';

class MyClass {
    method(): void {
        helper();
    }
}

export const instance = new MyClass();
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeFile))
	defer a.Close()

	graph, err := a.Analyze(context.Background(), []string{path}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if graph == nil {
		t.Fatal("Expected non-nil graph")
	}
}

func TestAnalyze_Java(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "Test.java")

	code := `package com.example;

import java.util.List;

public class Test {
    public void main(String[] args) {
        helper();
    }

    private void helper() {}
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeFile))
	defer a.Close()

	graph, err := a.Analyze(context.Background(), []string{path}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if graph == nil {
		t.Fatal("Expected non-nil graph")
	}
}

func TestAnalyze_Rust(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.rs")

	code := `use std::io;

fn main() {
    helper();
}

fn helper() {
    println!("Hello");
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithScope(ScopeFile))
	defer a.Close()

	graph, err := a.Analyze(context.Background(), []string{path}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if graph == nil {
		t.Fatal("Expected non-nil graph")
	}
}

func TestEdgeTypeArrows(t *testing.T) {
	g := NewDependencyGraph()
	g.AddNode(Node{ID: "a", Name: "a"})
	g.AddNode(Node{ID: "b", Name: "b"})
	g.AddNode(Node{ID: "c", Name: "c"})
	g.AddNode(Node{ID: "d", Name: "d"})
	g.AddNode(Node{ID: "e", Name: "e"})
	g.AddNode(Node{ID: "f", Name: "f"})

	g.AddEdge(Edge{From: "a", To: "b", Type: EdgeCall})
	g.AddEdge(Edge{From: "a", To: "c", Type: EdgeImport})
	g.AddEdge(Edge{From: "a", To: "d", Type: EdgeInherit})
	g.AddEdge(Edge{From: "a", To: "e", Type: EdgeImplement})
	g.AddEdge(Edge{From: "a", To: "f", Type: EdgeUses})

	mermaid := g.ToMermaid()

	if !contains(mermaid, "calls") {
		t.Error("Expected 'calls' label for EdgeCall")
	}
	if !contains(mermaid, "imports") {
		t.Error("Expected 'imports' label for EdgeImport")
	}
	if !contains(mermaid, "inherits") {
		t.Error("Expected 'inherits' label for EdgeInherit")
	}
	if !contains(mermaid, "implements") {
		t.Error("Expected 'implements' label for EdgeImplement")
	}
}

func TestComplexityColors(t *testing.T) {
	g := NewDependencyGraph()
	g.AddNode(Node{ID: "low", Name: "low"})
	g.AddNode(Node{ID: "medium", Name: "medium"})
	g.AddNode(Node{ID: "high", Name: "high"})
	g.AddNode(Node{ID: "critical", Name: "critical"})

	opts := MermaidOptions{
		ShowComplexity: true,
		NodeComplexity: map[string]int{
			"low":      2,
			"medium":   5,
			"high":     10,
			"critical": 20,
		},
	}

	mermaid := g.ToMermaidWithOptions(opts)

	if !contains(mermaid, "#90EE90") {
		t.Error("Expected green color for low complexity")
	}
	if !contains(mermaid, "#FFD700") {
		t.Error("Expected gold color for medium complexity")
	}
	if !contains(mermaid, "#FFA500") {
		t.Error("Expected orange color for high complexity")
	}
	if !contains(mermaid, "#FF6347") {
		t.Error("Expected red color for critical complexity")
	}
}

func TestCalculateMetrics_LargeGraph(t *testing.T) {
	a := New()
	defer a.Close()

	g := NewDependencyGraph()
	// Create a larger graph
	for i := 0; i < 20; i++ {
		id := string(rune('a' + i))
		g.AddNode(Node{ID: id, Name: id, Type: NodeFunction})
	}
	// Create a more complex edge pattern
	for i := 0; i < 19; i++ {
		from := string(rune('a' + i))
		to := string(rune('a' + i + 1))
		g.AddEdge(Edge{From: from, To: to, Type: EdgeCall})
	}
	// Add some cross edges
	g.AddEdge(Edge{From: "a", To: "j", Type: EdgeCall})
	g.AddEdge(Edge{From: "j", To: "t", Type: EdgeCall})

	metrics := a.CalculateMetrics(g)

	if metrics.Summary.TotalNodes != 20 {
		t.Errorf("TotalNodes = %d, want 20", metrics.Summary.TotalNodes)
	}
	if metrics.Summary.Density <= 0 {
		t.Error("Expected positive density")
	}
}

func TestAnalyze_EmptyFiles(t *testing.T) {
	a := New()
	defer a.Close()

	graph, err := a.Analyze(context.Background(), []string{}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(graph.Nodes) != 0 {
		t.Errorf("Expected 0 nodes, got %d", len(graph.Nodes))
	}
}

// Helper function to check if string contains substring
func contains(s, substr string) bool {
	return len(s) >= len(substr) && findSubstring(s, substr) >= 0
}

func findSubstring(s, substr string) int {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return i
		}
	}
	return -1
}
