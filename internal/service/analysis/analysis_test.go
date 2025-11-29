package analysis

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/internal/analyzer"
	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/models"
)

func TestNew(t *testing.T) {
	svc := New()
	if svc == nil {
		t.Fatal("New() returned nil")
	}
	if svc.config == nil {
		t.Error("config should not be nil")
	}
	if svc.opener == nil {
		t.Error("opener should not be nil")
	}
}

func TestNewWithConfig(t *testing.T) {
	cfg := &config.Config{}
	svc := New(WithConfig(cfg))
	if svc.config != cfg {
		t.Error("WithConfig did not set config")
	}
}

func TestNewWithOpener(t *testing.T) {
	mockOpener := mocks.NewMockOpener(t)
	svc := New(WithOpener(mockOpener))
	if svc.opener != mockOpener {
		t.Error("WithOpener did not set opener")
	}
}

func createTestGoFile(t *testing.T, content string) string {
	t.Helper()
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}
	return goFile
}

func TestAnalyzeComplexity(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func simple() {
	x := 1
	if x > 0 {
		x++
	}
}
`)

	svc := New()
	result, err := svc.AnalyzeComplexity([]string{goFile}, ComplexityOptions{})
	if err != nil {
		t.Fatalf("AnalyzeComplexity() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if len(result.Files) != 1 {
		t.Errorf("expected 1 file, got %d", len(result.Files))
	}
}

func TestAnalyzeComplexity_WithHalstead(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func add(a, b int) int {
	return a + b
}
`)

	svc := New()
	result, err := svc.AnalyzeComplexity([]string{goFile}, ComplexityOptions{
		IncludeHalstead: true,
	})
	if err != nil {
		t.Fatalf("AnalyzeComplexity() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeComplexity_WithProgress(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func test() {}
`)

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeComplexity([]string{goFile}, ComplexityOptions{
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeComplexity() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeSATD(t *testing.T) {
	goFile := createTestGoFile(t, `package main

// TODO: fix this
func broken() {
	// HACK: temporary workaround
}
`)

	svc := New()
	result, err := svc.AnalyzeSATD([]string{goFile}, SATDOptions{})
	if err != nil {
		t.Fatalf("AnalyzeSATD() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	// Should find at least the TODO and HACK comments
	if len(result.Items) == 0 {
		t.Error("expected at least one SATD item")
	}
}

func TestAnalyzeSATD_WithCustomPatterns(t *testing.T) {
	goFile := createTestGoFile(t, `package main

// CUSTOM: my pattern
func test() {}
`)

	svc := New()
	result, err := svc.AnalyzeSATD([]string{goFile}, SATDOptions{
		CustomPatterns: []PatternConfig{
			{Pattern: "CUSTOM:", Category: models.DebtDesign, Severity: models.SeverityHigh},
		},
	})
	if err != nil {
		t.Fatalf("AnalyzeSATD() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeSATD_InvalidPattern(t *testing.T) {
	goFile := createTestGoFile(t, `package main
func test() {}
`)

	svc := New()
	_, err := svc.AnalyzeSATD([]string{goFile}, SATDOptions{
		CustomPatterns: []PatternConfig{
			{Pattern: "[invalid", Category: models.DebtDesign, Severity: models.SeverityHigh},
		},
	})
	if err == nil {
		t.Error("expected error for invalid pattern")
	}

	var patternErr *PatternError
	if _, ok := err.(*PatternError); !ok {
		t.Errorf("expected *PatternError, got %T", patternErr)
	}
}

func TestAnalyzeDeadCode(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func unused() {
	x := 1
	_ = x
}
`)

	svc := New()
	result, err := svc.AnalyzeDeadCode([]string{goFile}, DeadCodeOptions{
		Confidence: 0.5,
	})
	if err != nil {
		t.Fatalf("AnalyzeDeadCode() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeDuplicates(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func dup1() {
	x := 1
	y := 2
	z := x + y
	_ = z
}

func dup2() {
	x := 1
	y := 2
	z := x + y
	_ = z
}
`)

	svc := New()
	result, err := svc.AnalyzeDuplicates([]string{goFile}, DuplicatesOptions{
		MinLines:            3,
		SimilarityThreshold: 0.8,
	})
	if err != nil {
		t.Fatalf("AnalyzeDuplicates() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeTDG(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte(`package main

func simple() {
	x := 1
	_ = x
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.AnalyzeTDG(tmpDir)
	if err != nil {
		t.Fatalf("AnalyzeTDG() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if result.TotalFiles == 0 {
		t.Error("expected at least one file")
	}
}

func TestAnalyzeGraph(t *testing.T) {
	goFile := createTestGoFile(t, `package main

import "fmt"

func test() {
	fmt.Println("hello")
}
`)

	svc := New()
	graph, metrics, err := svc.AnalyzeGraph([]string{goFile}, GraphOptions{
		Scope:          analyzer.ScopeFile,
		IncludeMetrics: true,
	})
	if err != nil {
		t.Fatalf("AnalyzeGraph() error = %v", err)
	}
	if graph == nil {
		t.Fatal("expected non-nil graph")
	}
	if metrics == nil {
		t.Error("expected metrics when IncludeMetrics is true")
	}
}

func TestAnalyzeGraph_NoMetrics(t *testing.T) {
	goFile := createTestGoFile(t, `package main
func test() {}
`)

	svc := New()
	graph, metrics, err := svc.AnalyzeGraph([]string{goFile}, GraphOptions{
		IncludeMetrics: false,
	})
	if err != nil {
		t.Fatalf("AnalyzeGraph() error = %v", err)
	}
	if graph == nil {
		t.Fatal("expected non-nil graph")
	}
	if metrics != nil {
		t.Error("expected nil metrics when IncludeMetrics is false")
	}
}

func TestAnalyzeCohesion(t *testing.T) {
	// Create a Java file for CK metrics (OO language)
	tmpDir := t.TempDir()
	javaFile := filepath.Join(tmpDir, "Test.java")
	if err := os.WriteFile(javaFile, []byte(`public class Test {
    private int x;

    public void setX(int x) {
        this.x = x;
    }

    public int getX() {
        return x;
    }
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.AnalyzeCohesion([]string{javaFile}, CohesionOptions{})
	if err != nil {
		t.Fatalf("AnalyzeCohesion() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeRepoMap(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func main() {
	helper()
}

func helper() {}
`)

	svc := New()
	result, err := svc.AnalyzeRepoMap([]string{goFile}, RepoMapOptions{Top: 10})
	if err != nil {
		t.Fatalf("AnalyzeRepoMap() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestPatternError(t *testing.T) {
	err := &PatternError{Pattern: "[bad", Err: os.ErrInvalid}
	if err.Error() == "" {
		t.Error("expected non-empty error message")
	}
	if err.Unwrap() != os.ErrInvalid {
		t.Error("Unwrap returned wrong error")
	}
}
