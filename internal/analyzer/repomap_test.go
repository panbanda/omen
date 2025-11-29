package analyzer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/models"
)

func TestNewRepoMapAnalyzer(t *testing.T) {
	a := NewRepoMapAnalyzer()
	defer a.Close()

	if a.graphAnalyzer == nil {
		t.Error("Expected graphAnalyzer to be initialized")
	}
}

func TestRepoMapAnalyzer_AnalyzeProject(t *testing.T) {
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

	analyzer := NewRepoMapAnalyzer()
	defer analyzer.Close()

	files := []string{file1, file2}
	repoMap, err := analyzer.AnalyzeProject(files)
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
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

func TestRepoMapAnalyzer_SortedByPageRank(t *testing.T) {
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

	analyzer := NewRepoMapAnalyzer()
	defer analyzer.Close()

	repoMap, err := analyzer.AnalyzeProject([]string{file1})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Verify sorting (should be descending by PageRank)
	for i := 0; i < len(repoMap.Symbols)-1; i++ {
		if repoMap.Symbols[i].PageRank < repoMap.Symbols[i+1].PageRank {
			t.Error("Symbols should be sorted by PageRank in descending order")
		}
	}
}

func TestRepoMapAnalyzer_EmptyProject(t *testing.T) {
	analyzer := NewRepoMapAnalyzer()
	defer analyzer.Close()

	repoMap, err := analyzer.AnalyzeProject([]string{})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(repoMap.Symbols) != 0 {
		t.Errorf("Expected 0 symbols for empty project, got %d", len(repoMap.Symbols))
	}
}

func TestGenerateSignature(t *testing.T) {
	tests := []struct {
		nodeType string
		name     string
		want     string
	}{
		{"function", "foo", "func foo()"},
		{"class", "MyClass", "class MyClass"},
		{"module", "mymod", "module mymod"},
		{"file", "main.go", "main.go"},
		{"unknown", "thing", "thing"},
	}

	for _, tt := range tests {
		t.Run(tt.nodeType, func(t *testing.T) {
			node := models.GraphNode{
				Type: models.NodeType(tt.nodeType),
				Name: tt.name,
			}
			got := generateSignature(node)
			if got != tt.want {
				t.Errorf("generateSignature() = %q, want %q", got, tt.want)
			}
		})
	}
}
