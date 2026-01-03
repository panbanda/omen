package analysis

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/analyzer/repomap"
)

func TestFocusedContext_IncludesCallers(t *testing.T) {
	// Setup: Create a Go project with caller/callee relationships
	tmpDir := t.TempDir()

	// File with the function we'll focus on
	mainCode := `package main

func main() {
	result := calculate(10)
	println(result)
}

func helper() {
	calculate(5)
}
`

	// File with the target function
	calcCode := `package main

func calculate(n int) int {
	return n * 2
}
`

	mainPath := filepath.Join(tmpDir, "main.go")
	calcPath := filepath.Join(tmpDir, "calc.go")

	err := os.WriteFile(mainPath, []byte(mainCode), 0644)
	if err != nil {
		t.Fatal(err)
	}
	err = os.WriteFile(calcPath, []byte(calcCode), 0644)
	if err != nil {
		t.Fatal(err)
	}

	// Build a repo map to enable symbol lookup
	rmAnalyzer := repomap.New()
	defer rmAnalyzer.Close()

	repoMap, err := rmAnalyzer.Analyze(context.Background(), []string{mainPath, calcPath})
	if err != nil {
		t.Fatalf("Failed to build repo map: %v", err)
	}

	svc := New()

	// Focus on the calculate function using repo map
	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:        "calculate",
		BaseDir:      tmpDir,
		IncludeGraph: true,
		RepoMap:      repoMap,
	})
	if err != nil {
		t.Fatalf("FocusedContext failed: %v", err)
	}

	// Verify callers are included
	if result.CallGraph == nil {
		t.Fatal("Expected CallGraph to be populated")
	}

	if len(result.CallGraph.Callers) == 0 {
		t.Error("Expected callers to be found for 'calculate'")
	}

	// Verify 'main' and 'helper' are listed as callers
	callerNames := make(map[string]bool)
	for _, caller := range result.CallGraph.Callers {
		callerNames[caller.Name] = true
	}

	if !callerNames["main"] {
		t.Error("Expected 'main' to be in callers list")
	}
	if !callerNames["helper"] {
		t.Error("Expected 'helper' to be in callers list")
	}
}

func TestFocusedContext_IncludesCallees(t *testing.T) {
	// Setup: Create a Go project where focused function calls others
	tmpDir := t.TempDir()

	code := `package main

func processor() {
	validate()
	transform()
	save()
}

func validate() {}
func transform() {}
func save() {}
`

	mainPath := filepath.Join(tmpDir, "main.go")
	err := os.WriteFile(mainPath, []byte(code), 0644)
	if err != nil {
		t.Fatal(err)
	}

	// Build a repo map to enable symbol lookup
	rmAnalyzer := repomap.New()
	defer rmAnalyzer.Close()

	repoMap, err := rmAnalyzer.Analyze(context.Background(), []string{mainPath})
	if err != nil {
		t.Fatalf("Failed to build repo map: %v", err)
	}

	svc := New()

	// Focus on the processor function
	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:        "processor",
		BaseDir:      tmpDir,
		IncludeGraph: true,
		RepoMap:      repoMap,
	})
	if err != nil {
		t.Fatalf("FocusedContext failed: %v", err)
	}

	// Verify callees are included
	if result.CallGraph == nil {
		t.Fatal("Expected CallGraph to be populated")
	}

	if len(result.CallGraph.Callees) == 0 {
		t.Error("Expected callees to be found for 'processor'")
	}

	// Verify 'validate', 'transform', 'save' are listed as callees
	calleeNames := make(map[string]bool)
	for _, callee := range result.CallGraph.Callees {
		calleeNames[callee.Name] = true
	}

	if !calleeNames["validate"] {
		t.Error("Expected 'validate' to be in callees list")
	}
	if !calleeNames["transform"] {
		t.Error("Expected 'transform' to be in callees list")
	}
	if !calleeNames["save"] {
		t.Error("Expected 'save' to be in callees list")
	}
}

func TestFocusedContext_FileWithCallGraph(t *testing.T) {
	// Test that focusing on a file also shows callers/callees for functions in that file
	tmpDir := t.TempDir()

	code := `package main

func main() {
	helper()
}

func helper() {
	println("hello")
}
`

	err := os.WriteFile(filepath.Join(tmpDir, "main.go"), []byte(code), 0644)
	if err != nil {
		t.Fatal(err)
	}

	svc := New()

	// Focus on the file
	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:        "main.go",
		BaseDir:      tmpDir,
		IncludeGraph: true,
	})
	if err != nil {
		t.Fatalf("FocusedContext failed: %v", err)
	}

	// For files, CallGraph should show internal calls
	if result.CallGraph == nil {
		t.Fatal("Expected CallGraph to be populated for file focus")
	}

	// main calls helper, so helper should have a caller
	if len(result.CallGraph.InternalCalls) == 0 {
		t.Error("Expected internal calls to be found in the file")
	}
}

func TestFocusedContext_GraphDisabledByDefault(t *testing.T) {
	// Verify that IncludeGraph=false (default) doesn't populate CallGraph
	tmpDir := t.TempDir()

	code := `package main

func main() {}
`

	mainPath := filepath.Join(tmpDir, "main.go")
	err := os.WriteFile(mainPath, []byte(code), 0644)
	if err != nil {
		t.Fatal(err)
	}

	// Build a repo map to enable symbol lookup
	rmAnalyzer := repomap.New()
	defer rmAnalyzer.Close()

	repoMap, err := rmAnalyzer.Analyze(context.Background(), []string{mainPath})
	if err != nil {
		t.Fatalf("Failed to build repo map: %v", err)
	}

	svc := New()

	result, err := svc.FocusedContext(context.Background(), FocusedContextOptions{
		Focus:   "main",
		BaseDir: tmpDir,
		RepoMap: repoMap,
		// IncludeGraph not set (defaults to false)
	})
	if err != nil {
		t.Fatalf("FocusedContext failed: %v", err)
	}

	// CallGraph should be nil when not requested
	if result.CallGraph != nil {
		t.Error("Expected CallGraph to be nil when IncludeGraph is false")
	}
}
