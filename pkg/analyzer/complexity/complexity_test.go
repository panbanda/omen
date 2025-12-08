package complexity

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
	a.Close()
}

func TestNewWithMaxFileSize(t *testing.T) {
	a := New(WithMaxFileSize(1024))
	if a == nil {
		t.Fatal("New(WithMaxFileSize) returned nil")
	}
	if a.maxFileSize != 1024 {
		t.Errorf("maxFileSize = %d, want 1024", a.maxFileSize)
	}
	a.Close()
}

func TestAnalyzeFile_Go(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	code := `package main

func simple() int {
	return 42
}

func withIf(x int) int {
	if x > 0 {
		return x
	}
	return 0
}

func nested(x, y int) int {
	if x > 0 {
		if y > 0 {
			return x + y
		}
	}
	return 0
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	result, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if result.Path != path {
		t.Errorf("Path = %q, want %q", result.Path, path)
	}

	if len(result.Functions) != 3 {
		t.Fatalf("len(Functions) = %d, want 3", len(result.Functions))
	}

	// Check simple function
	simple := result.Functions[0]
	if simple.Name != "simple" {
		t.Errorf("Functions[0].Name = %q, want %q", simple.Name, "simple")
	}
	if simple.Metrics.Cyclomatic != 1 {
		t.Errorf("simple.Cyclomatic = %d, want 1", simple.Metrics.Cyclomatic)
	}

	// Check withIf function
	withIf := result.Functions[1]
	if withIf.Name != "withIf" {
		t.Errorf("Functions[1].Name = %q, want %q", withIf.Name, "withIf")
	}
	if withIf.Metrics.Cyclomatic < 2 {
		t.Errorf("withIf.Cyclomatic = %d, want >= 2", withIf.Metrics.Cyclomatic)
	}

	// Check nested function has higher cognitive complexity
	nested := result.Functions[2]
	if nested.Name != "nested" {
		t.Errorf("Functions[2].Name = %q, want %q", nested.Name, "nested")
	}
	if nested.Metrics.Cognitive < withIf.Metrics.Cognitive {
		t.Errorf("nested.Cognitive (%d) should be >= withIf.Cognitive (%d)",
			nested.Metrics.Cognitive, withIf.Metrics.Cognitive)
	}
}

func TestAnalyzeFile_Python(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.py")

	code := `def simple():
    return 42

def with_if(x):
    if x > 0:
        return x
    return 0
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	result, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(result.Functions) != 2 {
		t.Fatalf("len(Functions) = %d, want 2", len(result.Functions))
	}

	if result.Functions[0].Name != "simple" {
		t.Errorf("Functions[0].Name = %q, want %q", result.Functions[0].Name, "simple")
	}
	if result.Functions[1].Name != "with_if" {
		t.Errorf("Functions[1].Name = %q, want %q", result.Functions[1].Name, "with_if")
	}
}

func TestAnalyzeFile_TypeScript(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.ts")

	code := `function simple(): number {
    return 42;
}

function withIf(x: number): number {
    if (x > 0) {
        return x;
    }
    return 0;
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	result, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(result.Functions) < 2 {
		t.Fatalf("len(Functions) = %d, want >= 2", len(result.Functions))
	}

	// Find withIf function and verify it has higher complexity
	var foundWithIf bool
	for _, fn := range result.Functions {
		if fn.Name == "withIf" {
			foundWithIf = true
			if fn.Metrics.Cyclomatic < 2 {
				t.Errorf("withIf.Cyclomatic = %d, want >= 2", fn.Metrics.Cyclomatic)
			}
		}
	}
	if !foundWithIf {
		t.Error("withIf function not found")
	}
}

func TestAnalyze(t *testing.T) {
	tmpDir := t.TempDir()

	// Create two test files
	file1 := filepath.Join(tmpDir, "a.go")
	code1 := `package main

func a1() int { return 1 }
func a2(x int) int {
	if x > 0 { return x }
	return 0
}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	file2 := filepath.Join(tmpDir, "b.go")
	code2 := `package main

func b1() int { return 2 }
`
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{file1, file2}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if analysis.Summary.TotalFiles != 2 {
		t.Errorf("TotalFiles = %d, want 2", analysis.Summary.TotalFiles)
	}

	if analysis.Summary.TotalFunctions != 3 {
		t.Errorf("TotalFunctions = %d, want 3", analysis.Summary.TotalFunctions)
	}
}

func TestAnalyzeFile_NonexistentFile(t *testing.T) {
	a := New()
	defer a.Close()

	_, err := a.AnalyzeFile("/nonexistent/path/file.go")
	if err == nil {
		t.Error("AnalyzeFile should fail for nonexistent file")
	}
}

func TestAnalyzeFile_UnsupportedLanguage(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.txt")

	if err := os.WriteFile(path, []byte("hello world"), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	_, err := a.AnalyzeFile(path)
	if err == nil {
		t.Error("AnalyzeFile should fail for unsupported language")
	}
}
