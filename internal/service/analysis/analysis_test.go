package analysis

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/internal/vcs/mocks"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/config"
)

func TestNew(t *testing.T) {
	svc := New()
	if svc == nil || svc.config == nil || svc.opener == nil {
		t.Fatal("New() returned nil or has nil config/opener")
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
	result, err := svc.AnalyzeComplexity(context.Background(), []string{goFile}, ComplexityOptions{})
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

func TestAnalyzeComplexity_WithProgress(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func test() {}
`)

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeComplexity(context.Background(), []string{goFile}, ComplexityOptions{
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
	result, err := svc.AnalyzeSATD(context.Background(), []string{goFile}, SATDOptions{})
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
	result, err := svc.AnalyzeSATD(context.Background(), []string{goFile}, SATDOptions{
		CustomPatterns: []PatternConfig{
			{Pattern: "CUSTOM:", Category: satd.CategoryDesign, Severity: satd.SeverityHigh},
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
	_, err := svc.AnalyzeSATD(context.Background(), []string{goFile}, SATDOptions{
		CustomPatterns: []PatternConfig{
			{Pattern: "[invalid", Category: satd.CategoryDesign, Severity: satd.SeverityHigh},
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
	result, err := svc.AnalyzeDeadCode(context.Background(), []string{goFile}, DeadCodeOptions{
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
	result, err := svc.AnalyzeDuplicates(context.Background(), []string{goFile}, DuplicatesOptions{
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
	result, err := svc.AnalyzeTDG(context.Background(), []string{goFile})
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
	graphResult, metrics, err := svc.AnalyzeGraph(context.Background(), []string{goFile}, GraphOptions{
		Scope:          graph.ScopeFile,
		IncludeMetrics: true,
	})
	if err != nil {
		t.Fatalf("AnalyzeGraph() error = %v", err)
	}
	if graphResult == nil {
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
	graphResult, metrics, err := svc.AnalyzeGraph(context.Background(), []string{goFile}, GraphOptions{
		IncludeMetrics: false,
	})
	if err != nil {
		t.Fatalf("AnalyzeGraph() error = %v", err)
	}
	if graphResult == nil {
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
	result, err := svc.AnalyzeCohesion(context.Background(), []string{javaFile}, CohesionOptions{})
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
	result, err := svc.AnalyzeRepoMap(context.Background(), []string{goFile}, RepoMapOptions{Top: 10})
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

// createTestGitRepo creates a temporary git repository with test files
func createTestGitRepo(t *testing.T) string {
	t.Helper()
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create and commit a test file
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte(`package main

func test() {
	x := 1
	if x > 0 {
		x++
	}
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	// Make a change and commit again
	if err := os.WriteFile(goFile, []byte(`package main

func test() {
	x := 1
	if x > 0 {
		x++
		x--
	}
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Second commit")

	return tmpDir
}

func runGit(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git %v failed: %v\n%s", args, err, out)
	}
}

func TestAnalyzeChurn(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	result, err := svc.AnalyzeChurn(context.Background(), repoPath, ChurnOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeChurn() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	// Should have at least one file with churn data
	if len(result.Files) == 0 {
		t.Error("expected at least one file with churn data")
	}
}

func TestAnalyzeChurn_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()

	svc := New()
	_, err := svc.AnalyzeChurn(context.Background(), tmpDir, ChurnOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

func TestAnalyzeChurn_DefaultDays(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	result, err := svc.AnalyzeChurn(context.Background(), repoPath, ChurnOptions{
		Days: 0, // Should use config default
	})
	if err != nil {
		t.Fatalf("AnalyzeChurn() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeDefects(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeDefects(context.Background(), repoPath, []string{goFile}, DefectOptions{})
	if err != nil {
		t.Fatalf("AnalyzeDefects() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeDefects_WithOptions(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeDefects(context.Background(), repoPath, []string{goFile}, DefectOptions{
		HighRiskOnly: true,
		ChurnDays:    90,
		MaxFileSize:  100000,
	})
	if err != nil {
		t.Fatalf("AnalyzeDefects() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeHotspots(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeHotspots(context.Background(), repoPath, []string{goFile}, HotspotOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeHotspots() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeHotspots_WithDays(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeHotspots(context.Background(), repoPath, []string{goFile}, HotspotOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeHotspots() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeHotspots_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	_, err := svc.AnalyzeHotspots(context.Background(), tmpDir, []string{goFile}, HotspotOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

func TestAnalyzeTemporalCoupling(t *testing.T) {
	repoPath := createTestGitRepo(t)

	// Add another file and commit both together
	file2 := filepath.Join(repoPath, "helper.go")
	if err := os.WriteFile(file2, []byte("package main\nfunc helper() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	// Update existing file
	goFile := filepath.Join(repoPath, "test.go")
	content, _ := os.ReadFile(goFile)
	if err := os.WriteFile(goFile, append(content, []byte("\n// comment")...), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, repoPath, "add", ".")
	runGit(t, repoPath, "commit", "-m", "Add helper and update test")

	svc := New()
	result, err := svc.AnalyzeTemporalCoupling(context.Background(), repoPath, TemporalCouplingOptions{
		Days:         365,
		MinCochanges: 1, // Lower threshold for testing
	})
	if err != nil {
		t.Fatalf("AnalyzeTemporalCoupling() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeTemporalCoupling_DefaultOptions(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	result, err := svc.AnalyzeTemporalCoupling(context.Background(), repoPath, TemporalCouplingOptions{
		Days:         0, // Should use default
		MinCochanges: 0, // Should use default
	})
	if err != nil {
		t.Fatalf("AnalyzeTemporalCoupling() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeTemporalCoupling_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()

	svc := New()
	_, err := svc.AnalyzeTemporalCoupling(context.Background(), tmpDir, TemporalCouplingOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

func TestAnalyzeOwnership(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeOwnership(context.Background(), repoPath, []string{goFile}, OwnershipOptions{})
	if err != nil {
		t.Fatalf("AnalyzeOwnership() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	// Should have ownership data for the file
	if len(result.Files) == 0 {
		t.Error("expected at least one file with ownership data")
	}
}

func TestAnalyzeOwnership_WithOptions(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeOwnership(context.Background(), repoPath, []string{goFile}, OwnershipOptions{
		Top:            10,
		IncludeTrivial: true,
	})
	if err != nil {
		t.Fatalf("AnalyzeOwnership() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeOwnership_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	_, err := svc.AnalyzeOwnership(context.Background(), tmpDir, []string{goFile}, OwnershipOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

// Additional tests to improve coverage

func TestAnalyzeComplexity_WithMaxFileSize(t *testing.T) {
	goFile := createTestGoFile(t, `package main
func test() {}
`)

	svc := New()
	result, err := svc.AnalyzeComplexity(context.Background(), []string{goFile}, ComplexityOptions{
		MaxFileSize: 10000,
	})
	if err != nil {
		t.Fatalf("AnalyzeComplexity() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeDeadCode_WithProgress(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func unused() {}
`)

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeDeadCode(context.Background(), []string{goFile}, DeadCodeOptions{
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeDeadCode() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeDeadCode_DefaultConfidence(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func unused() {}
`)

	svc := New()
	result, err := svc.AnalyzeDeadCode(context.Background(), []string{goFile}, DeadCodeOptions{
		Confidence: 0, // Should use config default
	})
	if err != nil {
		t.Fatalf("AnalyzeDeadCode() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeDuplicates_WithProgress(t *testing.T) {
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

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeDuplicates(context.Background(), []string{goFile}, DuplicatesOptions{
		MinLines:   3,
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeDuplicates() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeDuplicates_DefaultOptions(t *testing.T) {
	goFile := createTestGoFile(t, `package main
func test() {}
`)

	svc := New()
	result, err := svc.AnalyzeDuplicates(context.Background(), []string{goFile}, DuplicatesOptions{
		MinLines:            0, // Should use config default
		SimilarityThreshold: 0, // Should use config default
	})
	if err != nil {
		t.Fatalf("AnalyzeDuplicates() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeSATD_WithProgress(t *testing.T) {
	goFile := createTestGoFile(t, `package main

// TODO: fix this
func test() {}
`)

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeSATD(context.Background(), []string{goFile}, SATDOptions{
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeSATD() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeSATD_StrictMode(t *testing.T) {
	goFile := createTestGoFile(t, `package main

// TODO: fix this
func test() {}
`)

	svc := New()
	result, err := svc.AnalyzeSATD(context.Background(), []string{goFile}, SATDOptions{
		StrictMode: true,
	})
	if err != nil {
		t.Fatalf("AnalyzeSATD() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeGraph_WithProgress(t *testing.T) {
	goFile := createTestGoFile(t, `package main

import "fmt"

func test() {
	fmt.Println("hello")
}
`)

	progressCalled := false
	svc := New()
	graphResult, _, err := svc.AnalyzeGraph(context.Background(), []string{goFile}, GraphOptions{
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeGraph() error = %v", err)
	}
	if graphResult == nil {
		t.Fatal("expected non-nil graph")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeCohesion_WithProgress(t *testing.T) {
	tmpDir := t.TempDir()
	javaFile := filepath.Join(tmpDir, "Test.java")
	if err := os.WriteFile(javaFile, []byte(`public class Test {
    private int x;
    public void setX(int x) { this.x = x; }
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeCohesion(context.Background(), []string{javaFile}, CohesionOptions{
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeCohesion() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeCohesion_WithIncludeTests(t *testing.T) {
	tmpDir := t.TempDir()
	javaFile := filepath.Join(tmpDir, "Test.java")
	if err := os.WriteFile(javaFile, []byte(`public class Test {
    public void test() {}
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	result, err := svc.AnalyzeCohesion(context.Background(), []string{javaFile}, CohesionOptions{
		IncludeTests: true,
	})
	if err != nil {
		t.Fatalf("AnalyzeCohesion() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeChurn_WithSpinner(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	// Pass a nil spinner - just testing the branch
	result, err := svc.AnalyzeChurn(context.Background(), repoPath, ChurnOptions{
		Days:    365,
		Spinner: nil, // explicitly nil
	})
	if err != nil {
		t.Fatalf("AnalyzeChurn() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestSortFilesByHotspot(t *testing.T) {
	repoPath := createTestGitRepo(t)

	// Create a simple file (low complexity)
	simpleFile := filepath.Join(repoPath, "simple.go")
	if err := os.WriteFile(simpleFile, []byte("package main\nfunc simple() {}\n"), 0644); err != nil {
		t.Fatal(err)
	}

	// Create a complex file (high complexity with many branches)
	complexFile := filepath.Join(repoPath, "complex.go")
	complexCode := `package main

func complex(x, y, z int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				return x + y + z
			} else {
				return x + y
			}
		} else {
			return x
		}
	} else if y > 0 {
		return y
	} else if z > 0 {
		return z
	}
	return 0
}
`
	if err := os.WriteFile(complexFile, []byte(complexCode), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, repoPath, "add", ".")
	runGit(t, repoPath, "commit", "-m", "Add files")

	svc := New()
	files := []string{simpleFile, complexFile}

	sorted, err := svc.SortFilesByHotspot(context.Background(), repoPath, files, HotspotOptions{Days: 365})
	if err != nil {
		t.Fatalf("SortFilesByHotspot() error = %v", err)
	}

	// Complex file should be first (higher hotspot score)
	if len(sorted) != 2 {
		t.Fatalf("expected 2 files, got %d", len(sorted))
	}
	if sorted[0].Path != complexFile {
		t.Errorf("expected complex file first, got %s", sorted[0].Path)
	}
}

func TestAnalyzeScore(t *testing.T) {
	goFile := createTestGoFile(t, `package main

func simple() {
	x := 1
	if x > 0 {
		x++
	}
}
`)

	svc := New()
	result, err := svc.AnalyzeScore(context.Background(), []string{goFile}, ScoreOptions{})
	if err != nil {
		t.Fatalf("AnalyzeScore() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	// Score should be between 0 and 100
	if result.Score < 0 || result.Score > 100 {
		t.Errorf("score should be between 0 and 100, got %d", result.Score)
	}
}

func TestAnalyzeScore_WithChurnDays(t *testing.T) {
	goFile := createTestGoFile(t, `package main
func test() {}
`)

	svc := New()
	result, err := svc.AnalyzeScore(context.Background(), []string{goFile}, ScoreOptions{
		ChurnDays: 90,
	})
	if err != nil {
		t.Fatalf("AnalyzeScore() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeTrend(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	result, err := svc.AnalyzeTrend(context.Background(), repoPath, TrendOptions{
		Period: "monthly",
		Since:  "3m",
	})
	if err != nil {
		t.Fatalf("AnalyzeTrend() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeTrend_DefaultOptions(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	result, err := svc.AnalyzeTrend(context.Background(), repoPath, TrendOptions{})
	if err != nil {
		t.Fatalf("AnalyzeTrend() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeTrend_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()

	svc := New()
	_, err := svc.AnalyzeTrend(context.Background(), tmpDir, TrendOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

func TestAnalyzeChanges_ExcludePatterns(t *testing.T) {
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create files: test.go (included), README.md (excluded)
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}\n"), 0644); err != nil {
		t.Fatal(err)
	}

	readmeFile := filepath.Join(tmpDir, "README.md")
	if err := os.WriteFile(readmeFile, []byte("# Test\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	// Make changes to both files
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() { x := 1; _ = x }\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readmeFile, []byte("# Test\nUpdated\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Second commit")

	// Create config with exclude pattern for README.md
	cfg := config.DefaultConfig()
	cfg.Exclude.Patterns = []string{"README.md"}

	svc := New(WithConfig(cfg))
	result, err := svc.AnalyzeChanges(context.Background(), tmpDir, ChangesOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeChanges() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}

	// Check that no commit has README.md in FilesModified
	for _, c := range result.Commits {
		for _, f := range c.FilesModified {
			if f == "README.md" {
				t.Errorf("Commit %s has README.md in FilesModified, should be excluded", c.CommitHash)
			}
		}
	}

	// Check that test.go IS in at least one commit's FilesModified
	foundTestGo := false
	for _, c := range result.Commits {
		for _, f := range c.FilesModified {
			if f == "test.go" {
				foundTestGo = true
				break
			}
		}
		if foundTestGo {
			break
		}
	}
	if !foundTestGo {
		t.Error("test.go should be in at least one commit's FilesModified")
	}
}

func TestAnalyzeTemporalCoupling_ExcludePatterns(t *testing.T) {
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create files that will be committed together to create coupling
	goFile := filepath.Join(tmpDir, "main.go")
	helperFile := filepath.Join(tmpDir, "helper.go")
	readmeFile := filepath.Join(tmpDir, "README.md")

	// Commit 1: all files together
	if err := os.WriteFile(goFile, []byte("package main\nfunc main() {}\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(helperFile, []byte("package main\nfunc helper() {}\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readmeFile, []byte("# README\n"), 0644); err != nil {
		t.Fatal(err)
	}
	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Commit 1")

	// Commit 2: change all files together again
	if err := os.WriteFile(goFile, []byte("package main\nfunc main() { x := 1; _ = x }\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(helperFile, []byte("package main\nfunc helper() { y := 2; _ = y }\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readmeFile, []byte("# README\nUpdated\n"), 0644); err != nil {
		t.Fatal(err)
	}
	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Commit 2")

	// Commit 3: change all files together again (need 3 cochanges for default min)
	if err := os.WriteFile(goFile, []byte("package main\nfunc main() { x := 1; x++; _ = x }\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(helperFile, []byte("package main\nfunc helper() { y := 2; y++; _ = y }\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readmeFile, []byte("# README\nUpdated again\n"), 0644); err != nil {
		t.Fatal(err)
	}
	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Commit 3")

	// Create config with exclude pattern for README.md
	cfg := config.DefaultConfig()
	cfg.Exclude.Patterns = []string{"README.md"}

	svc := New(WithConfig(cfg))
	result, err := svc.AnalyzeTemporalCoupling(context.Background(), tmpDir, TemporalCouplingOptions{
		Days:         365,
		MinCochanges: 2, // Lower threshold to catch couplings
	})
	if err != nil {
		t.Fatalf("AnalyzeTemporalCoupling() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}

	// Check that no coupling involves README.md
	for _, c := range result.Couplings {
		if c.FileA == "README.md" || c.FileB == "README.md" {
			t.Errorf("Coupling involving README.md should be excluded: %s <-> %s", c.FileA, c.FileB)
		}
	}

	// Check that main.go <-> helper.go coupling IS present
	foundGoCouple := false
	for _, c := range result.Couplings {
		if (c.FileA == "main.go" && c.FileB == "helper.go") ||
			(c.FileA == "helper.go" && c.FileB == "main.go") {
			foundGoCouple = true
			break
		}
	}
	if !foundGoCouple {
		t.Error("Expected coupling between main.go and helper.go")
	}
}

func TestAnalyzeHotspots_NilFiles(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	// Pass nil for files - should analyze all churned files from git history
	result, err := svc.AnalyzeHotspots(context.Background(), repoPath, nil, HotspotOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeHotspots() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	// Should have at least one file (test.go from createTestGitRepo)
	if len(result.Files) == 0 {
		t.Error("expected at least one file in hotspot results when files=nil")
	}
}

func TestAnalyzeHotspots_ExcludePatterns(t *testing.T) {
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create files: test.go (included), README.md (excluded)
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte(`package main

func test() {
	x := 1
	if x > 0 {
		x++
	}
}
`), 0644); err != nil {
		t.Fatal(err)
	}

	readmeFile := filepath.Join(tmpDir, "README.md")
	if err := os.WriteFile(readmeFile, []byte("# Test\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	// Make changes to both files
	if err := os.WriteFile(goFile, []byte(`package main

func test() {
	x := 1
	if x > 0 {
		x++
		x--
	}
}
`), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readmeFile, []byte("# Test\nUpdated\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Second commit")

	// Create config with exclude pattern for README.md
	cfg := config.DefaultConfig()
	cfg.Exclude.Patterns = []string{"README.md"}

	svc := New(WithConfig(cfg))
	// Pass nil to analyze all churned files
	result, err := svc.AnalyzeHotspots(context.Background(), tmpDir, nil, HotspotOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeHotspots() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}

	// Check that README.md is NOT in results (excluded)
	for _, f := range result.Files {
		if f.Path == "README.md" || filepath.Base(f.Path) == "README.md" {
			t.Error("README.md should be excluded but was found in results")
		}
	}

	// Check that test.go IS in results
	found := false
	for _, f := range result.Files {
		if f.Path == "test.go" || filepath.Base(f.Path) == "test.go" {
			found = true
			break
		}
	}
	if !found {
		t.Error("test.go should be in results but was not found")
	}
}

func TestAnalyzeChurn_ExcludePatterns(t *testing.T) {
	tmpDir := t.TempDir()

	// Initialize git repo
	runGit(t, tmpDir, "init")
	runGit(t, tmpDir, "config", "user.email", "test@test.com")
	runGit(t, tmpDir, "config", "user.name", "Test User")

	// Create files: test.go (included), README.md and go.mod (excluded)
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}\n"), 0644); err != nil {
		t.Fatal(err)
	}

	readmeFile := filepath.Join(tmpDir, "README.md")
	if err := os.WriteFile(readmeFile, []byte("# Test\n"), 0644); err != nil {
		t.Fatal(err)
	}

	goModFile := filepath.Join(tmpDir, "go.mod")
	if err := os.WriteFile(goModFile, []byte("module test\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Initial commit")

	// Make changes to all files
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() { x := 1; _ = x }\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(readmeFile, []byte("# Test\nUpdated\n"), 0644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(goModFile, []byte("module test\ngo 1.21\n"), 0644); err != nil {
		t.Fatal(err)
	}

	runGit(t, tmpDir, "add", ".")
	runGit(t, tmpDir, "commit", "-m", "Second commit")

	// Create config with exclude patterns
	cfg := config.DefaultConfig()
	cfg.Exclude.Patterns = []string{"README.md", "go.mod"}

	svc := New(WithConfig(cfg))
	result, err := svc.AnalyzeChurn(context.Background(), tmpDir, ChurnOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeChurn() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}

	// Check that excluded files are not in results
	for _, f := range result.Files {
		if f.RelativePath == "README.md" {
			t.Error("README.md should be excluded but was found in results")
		}
		if f.RelativePath == "go.mod" {
			t.Error("go.mod should be excluded but was found in results")
		}
	}

	// Check that test.go IS in results
	found := false
	for _, f := range result.Files {
		if f.RelativePath == "test.go" {
			found = true
			break
		}
	}
	if !found {
		t.Error("test.go should be in results but was not found")
	}
}
