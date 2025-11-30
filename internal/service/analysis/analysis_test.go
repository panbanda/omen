package analysis

import (
	"os"
	"os/exec"
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
	result, err := svc.AnalyzeChurn(repoPath, ChurnOptions{
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
	_, err := svc.AnalyzeChurn(tmpDir, ChurnOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

func TestAnalyzeChurn_DefaultDays(t *testing.T) {
	repoPath := createTestGitRepo(t)

	svc := New()
	result, err := svc.AnalyzeChurn(repoPath, ChurnOptions{
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
	result, err := svc.AnalyzeDefects(repoPath, []string{goFile}, DefectOptions{})
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
	result, err := svc.AnalyzeDefects(repoPath, []string{goFile}, DefectOptions{
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
	result, err := svc.AnalyzeHotspots(repoPath, []string{goFile}, HotspotOptions{
		Days: 365,
	})
	if err != nil {
		t.Fatalf("AnalyzeHotspots() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}

func TestAnalyzeHotspots_WithProgress(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeHotspots(repoPath, []string{goFile}, HotspotOptions{
		Days:       365,
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeHotspots() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeHotspots_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	_, err := svc.AnalyzeHotspots(tmpDir, []string{goFile}, HotspotOptions{})
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
	result, err := svc.AnalyzeTemporalCoupling(repoPath, TemporalCouplingOptions{
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
	result, err := svc.AnalyzeTemporalCoupling(repoPath, TemporalCouplingOptions{
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
	_, err := svc.AnalyzeTemporalCoupling(tmpDir, TemporalCouplingOptions{})
	if err == nil {
		t.Error("expected error for non-git directory")
	}
}

func TestAnalyzeOwnership(t *testing.T) {
	repoPath := createTestGitRepo(t)
	goFile := filepath.Join(repoPath, "test.go")

	svc := New()
	result, err := svc.AnalyzeOwnership(repoPath, []string{goFile}, OwnershipOptions{})
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

	progressCalled := false
	svc := New()
	result, err := svc.AnalyzeOwnership(repoPath, []string{goFile}, OwnershipOptions{
		Top:            10,
		IncludeTrivial: true,
		OnProgress:     func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeOwnership() error = %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
	if !progressCalled {
		t.Error("expected progress callback to be called")
	}
}

func TestAnalyzeOwnership_NonGitDir(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte("package main\nfunc test() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	svc := New()
	_, err := svc.AnalyzeOwnership(tmpDir, []string{goFile}, OwnershipOptions{})
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
	result, err := svc.AnalyzeComplexity([]string{goFile}, ComplexityOptions{
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
	result, err := svc.AnalyzeDeadCode([]string{goFile}, DeadCodeOptions{
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
	result, err := svc.AnalyzeDeadCode([]string{goFile}, DeadCodeOptions{
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
	result, err := svc.AnalyzeDuplicates([]string{goFile}, DuplicatesOptions{
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
	result, err := svc.AnalyzeDuplicates([]string{goFile}, DuplicatesOptions{
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
	result, err := svc.AnalyzeSATD([]string{goFile}, SATDOptions{
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
	result, err := svc.AnalyzeSATD([]string{goFile}, SATDOptions{
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
	graph, _, err := svc.AnalyzeGraph([]string{goFile}, GraphOptions{
		OnProgress: func() { progressCalled = true },
	})
	if err != nil {
		t.Fatalf("AnalyzeGraph() error = %v", err)
	}
	if graph == nil {
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
	result, err := svc.AnalyzeCohesion([]string{javaFile}, CohesionOptions{
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
	result, err := svc.AnalyzeCohesion([]string{javaFile}, CohesionOptions{
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
	result, err := svc.AnalyzeChurn(repoPath, ChurnOptions{
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
