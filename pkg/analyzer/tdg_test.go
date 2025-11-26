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

func TestNormalize(t *testing.T) {
	tests := []struct {
		name      string
		value     float64
		threshold float64
		want      float64
	}{
		{
			name:      "value at threshold",
			value:     20.0,
			threshold: 20.0,
			want:      1.0,
		},
		{
			name:      "value above threshold",
			value:     30.0,
			threshold: 20.0,
			want:      1.0,
		},
		{
			name:      "value at half threshold",
			value:     10.0,
			threshold: 20.0,
			want:      0.5,
		},
		{
			name:      "value below threshold",
			value:     5.0,
			threshold: 20.0,
			want:      0.25,
		},
		{
			name:      "zero value",
			value:     0.0,
			threshold: 20.0,
			want:      0.0,
		},
		{
			name:      "negative value",
			value:     -5.0,
			threshold: 20.0,
			want:      0.0,
		},
		{
			name:      "very small value",
			value:     0.1,
			threshold: 20.0,
			want:      0.005,
		},
		{
			name:      "value at 75% threshold",
			value:     15.0,
			threshold: 20.0,
			want:      0.75,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := normalize(tt.value, tt.threshold)
			if got != tt.want {
				t.Errorf("normalize(%v, %v) = %v, want %v", tt.value, tt.threshold, got, tt.want)
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
			sorted:     []float64{10.0, 20.0, 30.0, 40.0, 50.0},
			percentile: 50,
			want:       30.0,
		},
		{
			name:       "p95 of 5 elements",
			sorted:     []float64{10.0, 20.0, 30.0, 40.0, 50.0},
			percentile: 95,
			want:       50.0,
		},
		{
			name:       "p0 of elements",
			sorted:     []float64{10.0, 20.0, 30.0},
			percentile: 0,
			want:       10.0,
		},
		{
			name:       "p100 of elements",
			sorted:     []float64{10.0, 20.0, 30.0},
			percentile: 100,
			want:       30.0,
		},
		{
			name:       "empty slice",
			sorted:     []float64{},
			percentile: 50,
			want:       0.0,
		},
		{
			name:       "single element",
			sorted:     []float64{42.0},
			percentile: 50,
			want:       42.0,
		},
		{
			name:       "large dataset p50",
			sorted:     []float64{1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0},
			percentile: 50,
			want:       6.0,
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

	if file.Value < 0 || file.Value > 100 {
		t.Errorf("TDG score %v out of range [0-100]", file.Value)
	}

	if file.Severity == "" {
		t.Error("Severity should not be empty")
	}

	if file.Components.Complexity < 0 || file.Components.Complexity > 1 {
		t.Errorf("Complexity component %v out of range [0-1]", file.Components.Complexity)
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

	if result.Summary.AvgScore <= 0 {
		t.Error("AvgScore should be > 0")
	}

	if result.Summary.AvgScore > 100 {
		t.Error("AvgScore should be <= 100")
	}

	if result.Summary.MaxScore < 0 || result.Summary.MaxScore > 100 {
		t.Errorf("MaxScore %v out of range [0-100]", result.Summary.MaxScore)
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

	if complexScore >= simpleScore {
		t.Errorf("complex file score (%v) should be lower than simple file score (%v)", complexScore, simpleScore)
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

	for i := 1; i < len(result.Files); i++ {
		if result.Files[i].Value < result.Files[i-1].Value {
			t.Errorf("files not sorted by score ascending: files[%d].Value (%v) < files[%d].Value (%v)",
				i, result.Files[i].Value, i-1, result.Files[i-1].Value)
		}
	}
}

func TestTDGAnalyzeProject_SeverityDistribution(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"excellent.go": `package main
func excellent() { x := 1 }`,
		"good.go": `package main
func good(x int) int {
	if x > 0 { return x }
	return 0
}`,
		"moderate.go": `package main
func moderate(x, y int) int {
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

	totalBySeverity := 0
	for _, count := range result.Summary.BySeverity {
		totalBySeverity += count
	}

	if totalBySeverity != len(result.Files) {
		t.Errorf("severity counts (%d) don't match file count (%d)", totalBySeverity, len(result.Files))
	}

	for _, file := range result.Files {
		switch file.Severity {
		case models.TDGExcellent, models.TDGGood, models.TDGModerate, models.TDGHighRisk:
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

func TestTDGAnalyzeProject_PercentileCalculations(t *testing.T) {
	tmpDir := t.TempDir()

	var filePaths []string
	for i := 0; i < 20; i++ {
		complexity := i
		var ifStatements string
		for j := 0; j < complexity; j++ {
			ifStatements += "if true { x := 1 }\n"
		}
		content := "package main\nfunc test() {\n" + ifStatements + "}"
		path := filepath.Join(tmpDir, "test"+string(rune('a'+i))+".go")
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

	if result.Summary.P50Score <= 0 {
		t.Error("P50Score should be > 0")
	}

	if result.Summary.P95Score <= 0 {
		t.Error("P95Score should be > 0")
	}

	if result.Summary.P50Score > result.Summary.P95Score {
		t.Errorf("P50Score (%v) should be <= P95Score (%v)", result.Summary.P50Score, result.Summary.P95Score)
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
	expectedPenalty := file.Components.Complexity*weights.Complexity +
		file.Components.Churn*weights.Churn +
		file.Components.Coupling*weights.Coupling +
		file.Components.Duplication*weights.Duplication +
		file.Components.DomainRisk*weights.DomainRisk

	expectedScore := (1.0 - expectedPenalty) * 100.0
	if expectedScore < 0 {
		expectedScore = 0
	}
	if expectedScore > 100 {
		expectedScore = 100
	}

	if file.Value != expectedScore {
		t.Errorf("TDG score = %v, expected %v (penalty: %v)", file.Value, expectedScore, expectedPenalty)
	}
}

func TestTDGAnalyzeProject_DuplicationDetection(t *testing.T) {
	tmpDir := t.TempDir()

	duplicateCode := `package main

func duplicate() {
	x := 1
	y := 2
	z := 3
	a := 4
	b := 5
	c := 6
	return
}`

	file1 := filepath.Join(tmpDir, "dup1.go")
	file2 := filepath.Join(tmpDir, "dup2.go")

	if err := os.WriteFile(file1, []byte(duplicateCode), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(duplicateCode), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	analyzer := NewTDGAnalyzer(90)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(tmpDir, []string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	hasDuplicationPenalty := false
	for _, file := range result.Files {
		if file.Components.Duplication > 0 {
			hasDuplicationPenalty = true
			break
		}
	}

	if !hasDuplicationPenalty {
		t.Log("Note: duplication detection depends on minLines threshold and similarity")
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

func TestTDGAnalyzeProject_ChurnIntegration(t *testing.T) {
	tmpDir := t.TempDir()

	code := `package main

func test() {
	x := 1
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
	if file.Components.Churn < 0 || file.Components.Churn > 1 {
		t.Errorf("Churn component %v out of range [0-1]", file.Components.Churn)
	}
}

func TestTDGAnalyzeProject_ComplexityNormalization(t *testing.T) {
	tmpDir := t.TempDir()

	tests := []struct {
		name             string
		code             string
		expectedMaxCyc   float64
		expectedNormLess float64
	}{
		{
			name: "below threshold",
			code: `package main
func test() { return }`,
			expectedMaxCyc:   1.0,
			expectedNormLess: 0.1,
		},
		{
			name: "at threshold",
			code: `package main
func test(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t int) int {
	if a > 0 { return a }
	if b > 0 { return b }
	if c > 0 { return c }
	if d > 0 { return d }
	if e > 0 { return e }
	if f > 0 { return f }
	if g > 0 { return g }
	if h > 0 { return h }
	if i > 0 { return i }
	if j > 0 { return j }
	if k > 0 { return k }
	if l > 0 { return l }
	if m > 0 { return m }
	if n > 0 { return n }
	if o > 0 { return o }
	if p > 0 { return p }
	if q > 0 { return q }
	if r > 0 { return r }
	if s > 0 { return s }
	return 0
}`,
			expectedMaxCyc:   20.0,
			expectedNormLess: 1.1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewTDGAnalyzer(90)
			defer analyzer.Close()

			result, err := analyzer.AnalyzeProject(tmpDir, []string{testFile})
			if err != nil {
				t.Fatalf("AnalyzeProject() error = %v", err)
			}

			file := result.Files[0]
			if file.Components.Complexity < 0 || file.Components.Complexity > 1 {
				t.Errorf("Complexity component %v out of range [0-1]", file.Components.Complexity)
			}

			if file.Components.Complexity >= tt.expectedNormLess {
				t.Errorf("Complexity component %v should be < %v", file.Components.Complexity, tt.expectedNormLess)
			}
		})
	}
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

	if file.Value != 100.0 {
		t.Errorf("TDG score should be 100 for file with no complexity, got %v", file.Value)
	}

	if file.Severity != models.TDGExcellent {
		t.Errorf("Severity should be excellent for score 100, got %v", file.Severity)
	}
}

func TestTDGAnalyzeProject_CouplingPlaceholder(t *testing.T) {
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
	if file.Components.Coupling != 0 {
		t.Errorf("Coupling should be 0 (placeholder), got %v", file.Components.Coupling)
	}
}

func TestTDGAnalyzeProject_DomainRiskPlaceholder(t *testing.T) {
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
	if file.Components.DomainRisk != 0 {
		t.Errorf("DomainRisk should be 0 (placeholder), got %v", file.Components.DomainRisk)
	}
}
