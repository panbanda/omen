package repomap

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer/graph"
)

func TestNew(t *testing.T) {
	a := New()
	defer a.Close()

	if a.graphAnalyzer == nil {
		t.Error("Expected graphAnalyzer to be initialized")
	}
}

func TestNew_WithMaxFileSize(t *testing.T) {
	maxSize := int64(1024)
	a := New(WithMaxFileSize(maxSize))
	defer a.Close()

	if a.maxFileSize != maxSize {
		t.Errorf("maxFileSize = %v, want %v", a.maxFileSize, maxSize)
	}
}

func TestAnalyzer_AnalyzeProject(t *testing.T) {
	// Create a temporary directory with Go source files
	tmpDir := t.TempDir()

	// Create file with functions
	file1 := filepath.Join(tmpDir, "main.go")
	content1 := `package main

func main() {
	helper()
}

func helper() {
	println("hello")
}
`
	if err := os.WriteFile(file1, []byte(content1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	// Create another file
	file2 := filepath.Join(tmpDir, "utils.go")
	content2 := `package main

func utilFunc() int {
	return 42
}
`
	if err := os.WriteFile(file2, []byte(content2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := New()
	defer analyzer.Close()

	files := []string{file1, file2}
	repoMap, err := analyzer.Analyze(context.Background(), files)
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(repoMap.Symbols) == 0 {
		t.Error("Expected at least one symbol")
	}

	// Check that symbols have the expected fields
	for _, s := range repoMap.Symbols {
		if s.Name == "" {
			t.Error("Symbol should have a name")
		}
		if s.Kind == "" {
			t.Error("Symbol should have a kind")
		}
		if s.File == "" {
			t.Error("Symbol should have a file")
		}
	}

	// Summary should be calculated
	if repoMap.Summary.TotalSymbols != len(repoMap.Symbols) {
		t.Errorf("Summary.TotalSymbols = %d, want %d", repoMap.Summary.TotalSymbols, len(repoMap.Symbols))
	}
}

func TestAnalyzer_SortedByPageRank(t *testing.T) {
	tmpDir := t.TempDir()

	// Create file with multiple functions
	file1 := filepath.Join(tmpDir, "main.go")
	content := `package main

func a() { b(); c() }
func b() { c() }
func c() {}
`
	if err := os.WriteFile(file1, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := New()
	defer analyzer.Close()

	repoMap, err := analyzer.Analyze(context.Background(), []string{file1})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Verify sorting (should be descending by PageRank)
	for i := 0; i < len(repoMap.Symbols)-1; i++ {
		if repoMap.Symbols[i].PageRank < repoMap.Symbols[i+1].PageRank {
			t.Error("Symbols should be sorted by PageRank in descending order")
		}
	}
}

func TestAnalyzer_EmptyProject(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	repoMap, err := analyzer.Analyze(context.Background(), []string{})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(repoMap.Symbols) != 0 {
		t.Errorf("Expected 0 symbols for empty project, got %d", len(repoMap.Symbols))
	}
}

func TestGenerateSignature(t *testing.T) {
	tests := []struct {
		nodeType graph.NodeType
		name     string
		want     string
	}{
		{graph.NodeFunction, "foo", "func foo()"},
		{graph.NodeClass, "MyClass", "class MyClass"},
		{graph.NodeModule, "mymod", "module mymod"},
		{graph.NodeFile, "main.go", "main.go"},
		{graph.NodeType("unknown"), "thing", "thing"},
	}

	for _, tt := range tests {
		t.Run(string(tt.nodeType), func(t *testing.T) {
			node := graph.Node{
				Type: tt.nodeType,
				Name: tt.name,
			}
			got := generateSignature(node)
			if got != tt.want {
				t.Errorf("generateSignature() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestMap_SortByPageRank(t *testing.T) {
	m := &Map{
		Symbols: []Symbol{
			{Name: "low", PageRank: 0.1},
			{Name: "high", PageRank: 0.9},
			{Name: "mid", PageRank: 0.5},
		},
	}

	m.SortByPageRank()

	if m.Symbols[0].Name != "high" {
		t.Errorf("First symbol = %q, want high", m.Symbols[0].Name)
	}
	if m.Symbols[1].Name != "mid" {
		t.Errorf("Second symbol = %q, want mid", m.Symbols[1].Name)
	}
	if m.Symbols[2].Name != "low" {
		t.Errorf("Third symbol = %q, want low", m.Symbols[2].Name)
	}
}

func TestMap_CalculateSummary(t *testing.T) {
	t.Run("with symbols", func(t *testing.T) {
		m := &Map{
			Symbols: []Symbol{
				{Name: "a", File: "a.go", PageRank: 0.8, InDegree: 3, OutDegree: 1},
				{Name: "b", File: "b.go", PageRank: 0.4, InDegree: 1, OutDegree: 2},
				{Name: "c", File: "a.go", PageRank: 0.2, InDegree: 0, OutDegree: 1},
			},
		}

		m.CalculateSummary()

		if m.Summary.TotalSymbols != 3 {
			t.Errorf("TotalSymbols = %d, want 3", m.Summary.TotalSymbols)
		}
		if m.Summary.TotalFiles != 2 {
			t.Errorf("TotalFiles = %d, want 2", m.Summary.TotalFiles)
		}
		if m.Summary.MaxPageRank != 0.8 {
			t.Errorf("MaxPageRank = %v, want 0.8", m.Summary.MaxPageRank)
		}
	})

	t.Run("empty symbols", func(t *testing.T) {
		m := &Map{Symbols: []Symbol{}}
		m.CalculateSummary()

		if m.Summary.TotalSymbols != 0 {
			t.Errorf("TotalSymbols = %d, want 0", m.Summary.TotalSymbols)
		}
	})
}

func TestMap_TopN(t *testing.T) {
	m := &Map{
		Symbols: []Symbol{
			{Name: "a", PageRank: 0.9},
			{Name: "b", PageRank: 0.7},
			{Name: "c", PageRank: 0.5},
			{Name: "d", PageRank: 0.3},
			{Name: "e", PageRank: 0.1},
		},
	}

	top3 := m.TopN(3)

	if len(top3) != 3 {
		t.Errorf("TopN(3) returned %d symbols, want 3", len(top3))
	}
	if top3[0].Name != "a" {
		t.Errorf("First = %q, want a", top3[0].Name)
	}
	if top3[2].Name != "c" {
		t.Errorf("Third = %q, want c", top3[2].Name)
	}
}

func TestMap_TopN_ExceedsLength(t *testing.T) {
	m := &Map{
		Symbols: []Symbol{
			{Name: "a", PageRank: 0.9},
			{Name: "b", PageRank: 0.7},
		},
	}

	top10 := m.TopN(10)

	if len(top10) != 2 {
		t.Errorf("TopN(10) returned %d symbols, want 2 (all available)", len(top10))
	}
}

func TestSymbol_Fields(t *testing.T) {
	s := Symbol{
		Name:      "testFunc",
		Kind:      "function",
		File:      "/path/to/file.go",
		Line:      42,
		Signature: "func testFunc()",
		PageRank:  0.75,
		InDegree:  5,
		OutDegree: 3,
	}

	if s.Name != "testFunc" {
		t.Errorf("Name = %q, want testFunc", s.Name)
	}
	if s.Kind != "function" {
		t.Errorf("Kind = %q, want function", s.Kind)
	}
	if s.Line != 42 {
		t.Errorf("Line = %d, want 42", s.Line)
	}
	if s.PageRank != 0.75 {
		t.Errorf("PageRank = %v, want 0.75", s.PageRank)
	}
}

func TestAnalyzer_AnalyzeWithProgress(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "main.go")
	content := `package main

func main() {}
`
	if err := os.WriteFile(file1, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := New()
	defer analyzer.Close()

	// Progress is now passed via context, but since the underlying graph analyzer
	// hasn't been refactored yet, we just test that Analyze works with a context
	ctx := context.Background()

	repoMap, err := analyzer.Analyze(ctx, []string{file1})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(repoMap.Symbols) == 0 {
		t.Error("Expected at least one symbol")
	}
}

func TestAnalyzer_Close(t *testing.T) {
	analyzer := New()
	analyzer.Close()

	// Should be safe to call multiple times
	analyzer.Close()
}

func TestAnalyzer_Close_NilGraphAnalyzer(t *testing.T) {
	// Create analyzer with nil graphAnalyzer
	analyzer := &Analyzer{graphAnalyzer: nil}
	analyzer.Close() // Should not panic
}
