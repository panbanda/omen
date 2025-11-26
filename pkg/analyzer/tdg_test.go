package analyzer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/models"
)

func TestNewTDGAnalyzer(t *testing.T) {
	tests := []struct {
		name      string
		churnDays int
	}{
		{
			name:      "default 90 days",
			churnDays: 90,
		},
		{
			name:      "30 days",
			churnDays: 30,
		},
		{
			name:      "180 days",
			churnDays: 180,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewTDGAnalyzer(tt.churnDays)
			defer analyzer.Close()

			if analyzer == nil {
				t.Fatal("NewTDGAnalyzer() returned nil")
			}
			if analyzer.complexity == nil {
				t.Error("complexity analyzer is nil")
			}
			if analyzer.churn == nil {
				t.Error("churn analyzer is nil")
			}
			if analyzer.duplicates == nil {
				t.Error("duplicates analyzer is nil")
			}
			if analyzer.weights != models.DefaultTDGWeights() {
				t.Error("weights not initialized to defaults")
			}
		})
	}
}

func TestClamp(t *testing.T) {
	tests := []struct {
		name  string
		value float64
		min   float64
		max   float64
		want  float64
	}{
		{
			name:  "value in range",
			value: 2.5,
			min:   0.0,
			max:   5.0,
			want:  2.5,
		},
		{
			name:  "value below min",
			value: -1.0,
			min:   0.0,
			max:   5.0,
			want:  0.0,
		},
		{
			name:  "value above max",
			value: 10.0,
			min:   0.0,
			max:   5.0,
			want:  5.0,
		},
		{
			name:  "value at min",
			value: 0.0,
			min:   0.0,
			max:   5.0,
			want:  0.0,
		},
		{
			name:  "value at max",
			value: 5.0,
			min:   0.0,
			max:   5.0,
			want:  5.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := clamp(tt.value, tt.min, tt.max)
			if got != tt.want {
				t.Errorf("clamp(%v, %v, %v) = %v, want %v", tt.value, tt.min, tt.max, got, tt.want)
			}
		})
	}
}

func TestCalculateDomainRisk(t *testing.T) {
	tests := []struct {
		name     string
		path     string
		minRisk  float64
		maxRisk  float64
	}{
		{
			name:    "auth path - high risk",
			path:    "/src/auth/login.go",
			minRisk: 2.0,
			maxRisk: 5.0,
		},
		{
			name:    "crypto path - high risk",
			path:    "/src/crypto/hash.go",
			minRisk: 2.0,
			maxRisk: 5.0,
		},
		{
			name:    "security path - high risk",
			path:    "/src/security/validator.go",
			minRisk: 2.0,
			maxRisk: 5.0,
		},
		{
			name:    "database path - medium risk",
			path:    "/src/database/connection.go",
			minRisk: 1.5,
			maxRisk: 5.0,
		},
		{
			name:    "migration path - medium risk",
			path:    "/src/migration/v1.go",
			minRisk: 1.5,
			maxRisk: 5.0,
		},
		{
			name:    "api path - lower risk",
			path:    "/src/api/handlers.go",
			minRisk: 1.0,
			maxRisk: 5.0,
		},
		{
			name:    "handler path - lower risk",
			path:    "/src/handler/user.go",
			minRisk: 1.0,
			maxRisk: 5.0,
		},
		{
			name:    "regular path - no domain risk",
			path:    "/src/utils/helpers.go",
			minRisk: 0.0,
			maxRisk: 0.0,
		},
		{
			name:    "combined auth+api - cumulative risk",
			path:    "/src/api/auth/token.go",
			minRisk: 3.0,
			maxRisk: 5.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := calculateDomainRisk(tt.path)
			if got < tt.minRisk || got > tt.maxRisk {
				t.Errorf("calculateDomainRisk(%q) = %v, want in range [%v, %v]", tt.path, got, tt.minRisk, tt.maxRisk)
			}
		})
	}
}

func TestPercentileFloat64TDG(t *testing.T) {
	tests := []struct {
		name       string
		sorted     []float64
		percentile int
		want       float64
	}{
		{
			name:       "p50 of 5 elements",
			sorted:     []float64{1.0, 2.0, 3.0, 4.0, 5.0},
			percentile: 50,
			want:       3.0,
		},
		{
			name:       "p95 of 5 elements",
			sorted:     []float64{1.0, 2.0, 3.0, 4.0, 5.0},
			percentile: 95,
			want:       5.0,
		},
		{
			name:       "p0 of elements",
			sorted:     []float64{1.0, 2.0, 3.0},
			percentile: 0,
			want:       1.0,
		},
		{
			name:       "p100 of elements",
			sorted:     []float64{1.0, 2.0, 3.0},
			percentile: 100,
			want:       3.0,
		},
		{
			name:       "empty slice",
			sorted:     []float64{},
			percentile: 50,
			want:       0.0,
		},
		{
			name:       "single element",
			sorted:     []float64{2.5},
			percentile: 50,
			want:       2.5,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := percentileFloat64TDG(tt.sorted, tt.percentile)
			if got != tt.want {
				t.Errorf("percentileFloat64TDG(%v, %d) = %v, want %v", tt.sorted, tt.percentile, got, tt.want)
			}
		})
	}
}

func TestTDGAnalyzeProject_SingleFile(t *testing.T) {
	tmpDir := t.TempDir()

	code := `package main

func simple() int {
	return 42
}`

	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if result == nil {
		t.Fatal("result is nil")
	}

	if len(result.Files) != 1 {
		t.Errorf("expected 1 file, got %d", len(result.Files))
	}

	if result.Summary.TotalFiles != 1 {
		t.Errorf("Summary.TotalFiles = %d, want 1", result.Summary.TotalFiles)
	}

	file := result.Files[0]
	if file.FilePath != testFile {
		t.Errorf("FilePath = %q, want %q", file.FilePath, testFile)
	}

	if file.Value < 0 || file.Value > 5 {
		t.Errorf("TDG score %v out of range [0-5]", file.Value)
	}

	if file.Severity == "" {
		t.Error("Severity should not be empty")
	}

	if file.Confidence <= 0 || file.Confidence > 1 {
		t.Errorf("Confidence %v out of range (0-1]", file.Confidence)
	}

	if file.Components.Complexity < 0 || file.Components.Complexity > 5 {
		t.Errorf("Complexity component %v out of range [0-5]", file.Components.Complexity)
	}
}

func TestTDGAnalyzeProject_MultipleFiles(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"simple.go": `package main

func simple() int {
	return 42
}`,
		"complex.go": `package main

func complex(x, y, z int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				for i := 0; i < 10; i++ {
					if i%2 == 0 {
						x += i
					}
				}
				return x + y + z
			}
			return x + y
		}
		return x
	}
	return 0
}`,
		"moderate.go": `package main

func moderate(x int) int {
	if x > 0 {
		return x * 2
	}
	return 0
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Files) != 3 {
		t.Errorf("expected 3 files, got %d", len(result.Files))
	}

	if result.Summary.TotalFiles != 3 {
		t.Errorf("Summary.TotalFiles = %d, want 3", result.Summary.TotalFiles)
	}

	if result.Summary.AvgScore < 0 {
		t.Error("AvgScore should be >= 0")
	}

	if result.Summary.AvgScore > 5 {
		t.Error("AvgScore should be <= 5")
	}

	if result.Summary.MaxScore < 0 || result.Summary.MaxScore > 5 {
		t.Errorf("MaxScore %v out of range [0-5]", result.Summary.MaxScore)
	}

	complexFile := filepath.Join(tmpDir, "complex.go")
	simpleFile := filepath.Join(tmpDir, "simple.go")

	var complexScore, simpleScore float64
	for _, f := range result.Files {
		if f.FilePath == complexFile {
			complexScore = f.Value
		}
		if f.FilePath == simpleFile {
			simpleScore = f.Value
		}
	}

	if complexScore <= simpleScore {
		t.Errorf("complex file score (%v) should be higher than simple file score (%v)", complexScore, simpleScore)
	}
}

func TestTDGAnalyzeProject_SortedByScore(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"simple.go": `package main
func simple() { return }`,
		"medium.go": `package main
func medium(x int) int {
	if x > 0 { return x }
	return 0
}`,
		"complex.go": `package main
func complex(x, y int) int {
	if x > 0 {
		if y > 0 { return x + y }
		return x
	}
	return 0
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	// Files should be sorted by score descending (highest debt first)
	for i := 1; i < len(result.Files); i++ {
		if result.Files[i].Value > result.Files[i-1].Value {
			t.Errorf("files not sorted by score descending: files[%d].Value (%v) > files[%d].Value (%v)",
				i, result.Files[i].Value, i-1, result.Files[i-1].Value)
		}
	}
}

func TestTDGAnalyzeProject_SeverityDistribution(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"normal.go": `package main
func normal() { x := 1 }`,
		"moderate.go": `package main
func moderate(x int) int {
	if x > 0 { return x }
	return 0
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	totalBySeverity := 0
	for _, count := range result.Summary.BySeverity {
		totalBySeverity += count
	}

	if totalBySeverity != len(result.Files) {
		t.Errorf("severity counts (%d) don't match file count (%d)", totalBySeverity, len(result.Files))
	}

	for _, file := range result.Files {
		switch file.Severity {
		case models.TDGNormal, models.TDGWarning, models.TDGCritical:
		default:
			t.Errorf("invalid severity: %v", file.Severity)
		}
	}
}

func TestTDGAnalyzeProject_Hotspots(t *testing.T) {
	tmpDir := t.TempDir()

	var filePaths []string
	for i := 0; i < 15; i++ {
		content := `package main
func test() { x := 1 }`
		path := filepath.Join(tmpDir, "test"+string(rune('0'+i))+".go")
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write file: %v", err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Summary.Hotspots) != 10 {
		t.Errorf("expected 10 hotspots, got %d", len(result.Summary.Hotspots))
	}

	for i := 0; i < len(result.Summary.Hotspots); i++ {
		if result.Summary.Hotspots[i].FilePath != result.Files[i].FilePath {
			t.Errorf("hotspot[%d] doesn't match worst file[%d]", i, i)
		}
	}
}

func TestTDGAnalyzeProject_FewFilesHotspots(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"file1.go": `package main
func test1() { x := 1 }`,
		"file2.go": `package main
func test2() { y := 2 }`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Summary.Hotspots) != 2 {
		t.Errorf("expected 2 hotspots for 2 files, got %d", len(result.Summary.Hotspots))
	}
}

func TestTDGAnalyzeProject_EmptyFileList(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Files) != 0 {
		t.Errorf("expected 0 files, got %d", len(result.Files))
	}

	if result.Summary.TotalFiles != 0 {
		t.Errorf("Summary.TotalFiles = %d, want 0", result.Summary.TotalFiles)
	}

	if result.Summary.AvgScore != 0 {
		t.Errorf("AvgScore should be 0 for empty project, got %v", result.Summary.AvgScore)
	}
}

func TestTDGAnalyzeProject_ComponentWeighting(t *testing.T) {
	tmpDir := t.TempDir()

	code := `package main

func test(x int) int {
	if x > 0 {
		return x
	}
	return 0
}`

	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	file := result.Files[0]

	weights := models.DefaultTDGWeights()
	expectedScore := file.Components.Complexity*weights.Complexity +
		file.Components.Churn*weights.Churn +
		file.Components.Coupling*weights.Coupling +
		file.Components.Duplication*weights.Duplication +
		file.Components.DomainRisk*weights.DomainRisk

	if expectedScore < 0 {
		expectedScore = 0
	}
	if expectedScore > 5 {
		expectedScore = 5
	}

	if file.Value != expectedScore {
		t.Errorf("TDG score = %v, expected %v", file.Value, expectedScore)
	}
}

func TestTDGAnalyzeProject_MaxScoreTracking(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"best.go": `package main
func best() { x := 1 }`,
		"worst.go": `package main
func worst(a, b, c, d int) int {
	if a > 0 {
		if b > 0 {
			if c > 0 {
				if d > 0 {
					return a + b + c + d
				}
				return a + b + c
			}
			return a + b
		}
		return a
	}
	return 0
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	// MaxScore should be the highest (worst) debt score
	if result.Summary.MaxScore != result.Files[0].Value {
		t.Errorf("MaxScore (%v) should equal worst file score (%v)", result.Summary.MaxScore, result.Files[0].Value)
	}
}

func TestTDGAnalyzer_Close(t *testing.T) {
	analyzer := NewTDGAnalyzer(90)

	analyzer.Close()

	defer func() {
		if r := recover(); r != nil {
			t.Error("Close() should not panic when called multiple times")
		}
	}()
	analyzer.Close()
}

func TestTDGAnalyzeProject_NoComplexityMetrics(t *testing.T) {
	tmpDir := t.TempDir()

	testFile := filepath.Join(tmpDir, "empty.go")
	if err := os.WriteFile(testFile, []byte("package main"), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	file := result.Files[0]
	if file.Components.Complexity != 0 {
		t.Errorf("Complexity should be 0 for file with no functions, got %v", file.Components.Complexity)
	}

	// Score may not be exactly 0 due to coupling estimation
	if file.Value < 0 || file.Value > 1.0 {
		t.Errorf("TDG score should be low for file with no complexity, got %v", file.Value)
	}

	if file.Severity != models.TDGNormal {
		t.Errorf("Severity should be normal for low score, got %v", file.Severity)
	}
}

func TestTDGAnalyzeProject_DomainRiskDetection(t *testing.T) {
	tmpDir := t.TempDir()

	authDir := filepath.Join(tmpDir, "auth")
	if err := os.MkdirAll(authDir, 0755); err != nil {
		t.Fatalf("failed to create auth dir: %v", err)
	}

	code := `package auth
func login() { return }`

	testFile := filepath.Join(authDir, "login.go")
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	file := result.Files[0]
	if file.Components.DomainRisk == 0 {
		t.Error("DomainRisk should be > 0 for auth path")
	}
}

func TestTDGAnalyzeProject_ConfidenceReduction(t *testing.T) {
	tmpDir := t.TempDir()

	code := `package main
func test() { x := 1 }`

	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	file := result.Files[0]
	// Confidence should be reduced when churn data is missing (not a git repo)
	if file.Confidence >= 1.0 {
		t.Errorf("Confidence should be < 1.0 when data is missing, got %v", file.Confidence)
	}
}
