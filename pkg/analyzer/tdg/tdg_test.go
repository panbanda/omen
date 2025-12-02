package tdg

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNew(t *testing.T) {
	t.Run("default options", func(t *testing.T) {
		analyzer := New()
		defer analyzer.Close()

		if analyzer.maxFileSize != 0 {
			t.Errorf("maxFileSize = %v, want 0", analyzer.maxFileSize)
		}

		if analyzer.config.Weights.StructuralComplexity == 0 {
			t.Error("config should be set to defaults")
		}
	})

	t.Run("with max file size", func(t *testing.T) {
		maxSize := int64(1024)
		analyzer := New(WithMaxFileSize(maxSize))
		defer analyzer.Close()

		if analyzer.maxFileSize != maxSize {
			t.Errorf("maxFileSize = %v, want %v", analyzer.maxFileSize, maxSize)
		}
	})

	t.Run("with custom config", func(t *testing.T) {
		customConfig := DefaultConfig()
		customConfig.Weights.StructuralComplexity = 999

		analyzer := New(WithConfig(customConfig))
		defer analyzer.Close()

		if analyzer.config.Weights.StructuralComplexity != 999 {
			t.Errorf("StructuralComplexity = %v, want 999", analyzer.config.Weights.StructuralComplexity)
		}
	})

	t.Run("with multiple options", func(t *testing.T) {
		customConfig := DefaultConfig()
		customConfig.Weights.StructuralComplexity = 999
		maxSize := int64(2048)

		analyzer := New(
			WithConfig(customConfig),
			WithMaxFileSize(maxSize),
		)
		defer analyzer.Close()

		if analyzer.config.Weights.StructuralComplexity != 999 {
			t.Errorf("StructuralComplexity = %v, want 999", analyzer.config.Weights.StructuralComplexity)
		}
		if analyzer.maxFileSize != maxSize {
			t.Errorf("maxFileSize = %v, want %v", analyzer.maxFileSize, maxSize)
		}
	})
}

func TestAnalyzer_AnalyzeSource(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	source := `
// A simple function
func simple() int {
    return 42
}
`

	score, err := analyzer.AnalyzeSource(source, LanguageGo, "test.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	if score.Language != LanguageGo {
		t.Errorf("Language = %v, want Go", score.Language)
	}

	if score.Total <= 0 || score.Total > 100 {
		t.Errorf("Total = %v, want 0-100", score.Total)
	}

	if score.Confidence <= 0 || score.Confidence > 1 {
		t.Errorf("Confidence = %v, want 0-1", score.Confidence)
	}
}

func TestAnalyzer_ComplexCode(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Complex nested code
	source := `
func complex(x int) int {
    if x > 0 {
        if x > 10 {
            if x > 20 {
                if x > 30 {
                    if x > 40 {
                        return x * 2
                    }
                }
            }
        }
    }
    return x
}
`

	score, err := analyzer.AnalyzeSource(source, LanguageGo, "complex.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// Deep nesting should result in penalties
	if score.SemanticComplexity >= 15.0 {
		t.Errorf("Deep nesting should reduce SemanticComplexity, got %v", score.SemanticComplexity)
	}

	if len(score.PenaltiesApplied) == 0 {
		t.Error("Complex code should have penalties applied")
	}
}

func TestAnalyzer_DuplicateCode(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Code with duplicates (same line repeated)
	source := `
func duplicate() {
    processDataWithLongFunctionName()
    processDataWithLongFunctionName()
    processDataWithLongFunctionName()
    processDataWithLongFunctionName()
    processDataWithLongFunctionName()
    doSomethingElse()
    doSomethingElse()
    doSomethingElse()
}
`

	score, err := analyzer.AnalyzeSource(source, LanguageGo, "dup.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// Duplication should be detected
	if score.DuplicationRatio == 15.0 {
		t.Error("Duplicated code should reduce duplication score")
	}
}

func TestAnalyzer_HighCoupling(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Many imports = high coupling
	source := `
import "fmt"
import "os"
import "io"
import "net/http"
import "encoding/json"
import "database/sql"
import "crypto/tls"
import "sync"
import "context"
import "time"
import "strings"
import "bytes"
import "errors"
import "log"
import "path/filepath"
import "regexp"
import "sort"
import "strconv"
import "testing"
import "reflect"
import "math"
import "bufio"
import "compress/gzip"
import "archive/zip"
import "html/template"

func main() {
    fmt.Println("hello")
}
`

	score, err := analyzer.AnalyzeSource(source, LanguageGo, "coupling.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// High coupling should reduce coupling score
	if score.CouplingScore >= 15.0 {
		t.Errorf("High coupling should reduce CouplingScore, got %v", score.CouplingScore)
	}
}

func TestAnalyzer_Documentation(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Well-documented code
	source := `
// Package main provides the entry point.
//
// This is a comprehensive documentation comment
// that spans multiple lines to demonstrate
// good documentation practices.
package main

// Main is the entry point of the application.
// It initializes all components and starts the server.
func main() {
    // Initialize
    init()
}

// init performs initialization.
func init() {
    // Setup
}
`

	score, err := analyzer.AnalyzeSource(source, LanguageGo, "doc.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// Good documentation should give points
	if score.DocCoverage <= 0 {
		t.Errorf("Well-documented code should have DocCoverage > 0, got %v", score.DocCoverage)
	}
}

func TestAnalyzer_CriticalDefects(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Rust code with .unwrap()
	source := `
fn main() {
    let value = some_result().unwrap();
    let other = another_result().unwrap();
    panic!("something went wrong");
}
`

	score, err := analyzer.AnalyzeSource(source, LanguageRust, "defects.rs")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	if !score.HasCriticalDefects {
		t.Error("Should detect critical defects (.unwrap() and panic!)")
	}

	if score.CriticalDefectsCount < 3 {
		t.Errorf("CriticalDefectsCount = %v, want >= 3", score.CriticalDefectsCount)
	}

	// Critical defects should result in F grade
	if score.Grade != GradeF {
		t.Errorf("Critical defects should result in Grade F, got %v", score.Grade)
	}

	if score.Total != 0 {
		t.Errorf("Critical defects should result in Total 0, got %v", score.Total)
	}
}

func TestAnalyzer_AnalyzeFile(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Create a temp file
	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")

	content := `
package main

func main() {
    println("hello")
}
`
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write temp file: %v", err)
	}

	score, err := analyzer.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile() error = %v", err)
	}

	if score.FilePath != path {
		t.Errorf("FilePath = %v, want %v", score.FilePath, path)
	}

	if score.Language != LanguageGo {
		t.Errorf("Language = %v, want Go", score.Language)
	}
}

func TestAnalyzer_AnalyzeProject(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	// Create temp directory with files
	dir := t.TempDir()

	files := map[string]string{
		"main.go":       `package main; func main() {}`,
		"helper.go":     `package main; func helper() {}`,
		"utils/util.go": `package utils; func Util() {}`,
	}

	for name, content := range files {
		path := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(path), 0755); err != nil {
			t.Fatalf("Failed to create dir: %v", err)
		}
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("Failed to write file: %v", err)
		}
	}

	project, err := analyzer.AnalyzeProject(dir)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if project.TotalFiles != 3 {
		t.Errorf("TotalFiles = %v, want 3", project.TotalFiles)
	}

	if project.LanguageDistribution[LanguageGo] != 3 {
		t.Errorf("Go files = %v, want 3", project.LanguageDistribution[LanguageGo])
	}
}

func TestAnalyzer_Compare(t *testing.T) {
	analyzer := New()
	defer analyzer.Close()

	dir := t.TempDir()

	// Simple file
	path1 := filepath.Join(dir, "simple.go")
	content1 := `package main; func main() {}`
	if err := os.WriteFile(path1, []byte(content1), 0644); err != nil {
		t.Fatalf("Failed to write file: %v", err)
	}

	// Much more complex file with deep nesting to trigger penalties
	path2 := filepath.Join(dir, "complex.go")
	content2 := `package main

import "fmt"
import "os"
import "io"
import "net"
import "log"
import "sync"
import "time"
import "strings"
import "bytes"
import "errors"
import "context"
import "path/filepath"
import "encoding/json"
import "net/http"
import "database/sql"
import "crypto/tls"
import "reflect"
import "runtime"
import "sort"
import "strconv"
import "archive/zip"

func complex(x int) {
    if x > 0 {
        if x > 10 {
            if x > 20 {
                if x > 30 {
                    if x > 40 {
                        if x > 50 {
                            fmt.Println(x)
                        }
                    }
                }
            }
        }
    }
    for i := 0; i < 10; i++ {
        for j := 0; j < 10; j++ {
            if i > j && j > 0 || i < 5 {
                println(i, j)
            }
        }
    }
}
`
	if err := os.WriteFile(path2, []byte(content2), 0644); err != nil {
		t.Fatalf("Failed to write file: %v", err)
	}

	comparison, err := analyzer.Compare(path1, path2)
	if err != nil {
		t.Fatalf("Compare() error = %v", err)
	}

	t.Logf("Source1 (simple) = %v, Source2 (complex) = %v, Delta = %v",
		comparison.Source1.Total, comparison.Source2.Total, comparison.Delta)

	// Both files should have scores between 0 and 100
	if comparison.Source1.Total < 0 || comparison.Source1.Total > 100 {
		t.Errorf("Source1.Total = %v, want 0-100", comparison.Source1.Total)
	}
	if comparison.Source2.Total < 0 || comparison.Source2.Total > 100 {
		t.Errorf("Source2.Total = %v, want 0-100", comparison.Source2.Total)
	}
}

func TestAnalyzer_SkipDirectories(t *testing.T) {
	analyzer := New()

	skipDirs := []string{
		"node_modules", "target", "build", "dist", ".git",
		"__pycache__", ".pytest_cache", "venv", ".venv",
		"vendor", ".idea", ".vscode",
	}

	for _, dir := range skipDirs {
		if !analyzer.shouldSkipDirectory(dir) {
			t.Errorf("shouldSkipDirectory(%q) = false, want true", dir)
		}
	}

	if analyzer.shouldSkipDirectory("src") {
		t.Error("shouldSkipDirectory(src) = true, want false")
	}
}

func TestAnalyzer_ShouldAnalyzeFile(t *testing.T) {
	analyzer := New()

	analyzeFiles := []string{
		"main.go", "app.rs", "script.py", "index.js", "app.ts",
		"component.tsx", "Main.java", "lib.c", "impl.cpp",
		"server.rb", "api.swift", "data.kt",
	}

	for _, file := range analyzeFiles {
		if !analyzer.shouldAnalyzeFile(file) {
			t.Errorf("shouldAnalyzeFile(%q) = false, want true", file)
		}
	}

	skipFiles := []string{
		"README.md", "config.yaml", "data.json", "Makefile",
		"image.png", "styles.css", "index.html",
	}

	for _, file := range skipFiles {
		if analyzer.shouldAnalyzeFile(file) {
			t.Errorf("shouldAnalyzeFile(%q) = true, want false", file)
		}
	}
}

func TestAnalyzer_EstimateCyclomaticComplexity(t *testing.T) {
	analyzer := New()

	tests := []struct {
		name        string
		source      string
		minExpected uint32
	}{
		{
			name:        "simple",
			source:      "func main() {\n    return\n}",
			minExpected: 1,
		},
		{
			name:        "with_if",
			source:      "func main() {\n    if x > 0 {\n        return\n    }\n}",
			minExpected: 2,
		},
		{
			name:        "with_for",
			source:      "func main() {\n    for i := 0; i < 10; i++ {\n        continue\n    }\n}",
			minExpected: 2,
		},
		{
			name:        "with_logical_ops",
			source:      "func main() {\n    if x > 0 && y > 0 || z > 0 {\n        return\n    }\n}",
			minExpected: 3,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			lines := strings.Split(tc.source, "\n")
			got := analyzer.estimateCyclomaticComplexity(lines)
			if got < tc.minExpected {
				t.Errorf("estimateCyclomaticComplexity() = %v, want >= %v", got, tc.minExpected)
			}
		})
	}
}

func TestAnalyzer_EstimateNestingDepth(t *testing.T) {
	analyzer := New()

	tests := []struct {
		name     string
		source   string
		expected int
	}{
		{
			name:     "flat",
			source:   "func main() {\n    return\n}",
			expected: 1,
		},
		{
			name:     "nested_2",
			source:   "func main() {\n    if x {\n        y\n    }\n}",
			expected: 2,
		},
		{
			name:     "nested_3",
			source:   "func main() {\n    if x {\n        for {\n            y\n        }\n    }\n}",
			expected: 3,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := analyzer.estimateNestingDepth(tc.source)
			if got != tc.expected {
				t.Errorf("estimateNestingDepth() = %v, want %v", got, tc.expected)
			}
		})
	}
}

func TestAnalyzer_MaxFileSize(t *testing.T) {
	dir := t.TempDir()

	smallFile := filepath.Join(dir, "small.go")
	smallContent := `package main; func main() {}`
	if err := os.WriteFile(smallFile, []byte(smallContent), 0644); err != nil {
		t.Fatalf("Failed to write small file: %v", err)
	}

	largeFile := filepath.Join(dir, "large.go")
	largeContent := `package main
func main() {
` + string(make([]byte, 2000)) + `
}`
	if err := os.WriteFile(largeFile, []byte(largeContent), 0644); err != nil {
		t.Fatalf("Failed to write large file: %v", err)
	}

	t.Run("no_limit", func(t *testing.T) {
		analyzer := New()
		defer analyzer.Close()

		_, err := analyzer.AnalyzeFile(smallFile)
		if err != nil {
			t.Errorf("AnalyzeFile(small) error = %v, want nil", err)
		}

		_, err = analyzer.AnalyzeFile(largeFile)
		if err != nil {
			t.Errorf("AnalyzeFile(large) error = %v, want nil", err)
		}
	})

	t.Run("with_limit", func(t *testing.T) {
		analyzer := New(WithMaxFileSize(100))
		defer analyzer.Close()

		_, err := analyzer.AnalyzeFile(smallFile)
		if err != nil {
			t.Errorf("AnalyzeFile(small) error = %v, want nil", err)
		}

		_, err = analyzer.AnalyzeFile(largeFile)
		if err == nil {
			t.Error("AnalyzeFile(large) should fail when file exceeds maxFileSize")
		}
	})
}

func TestGradeFromScore(t *testing.T) {
	tests := []struct {
		score float32
		want  Grade
	}{
		{96.0, GradeAPlus},
		{92.0, GradeA},
		{87.0, GradeAMinus},
		{82.0, GradeBPlus},
		{77.0, GradeB},
		{72.0, GradeBMinus},
		{67.0, GradeCPlus},
		{62.0, GradeC},
		{57.0, GradeCMinus},
		{52.0, GradeD},
		{40.0, GradeF},
	}

	for _, tt := range tests {
		got := GradeFromScore(tt.score)
		if got != tt.want {
			t.Errorf("GradeFromScore(%v) = %v, want %v", tt.score, got, tt.want)
		}
	}
}

func TestLanguageFromExtension(t *testing.T) {
	tests := []struct {
		path string
		want Language
	}{
		{"main.rs", LanguageRust},
		{"main.go", LanguageGo},
		{"script.py", LanguagePython},
		{"app.js", LanguageJavaScript},
		{"app.ts", LanguageTypeScript},
		{"app.tsx", LanguageTypeScript},
		{"Main.java", LanguageJava},
		{"lib.c", LanguageC},
		{"lib.cpp", LanguageCpp},
		{"app.cs", LanguageCSharp},
		{"app.rb", LanguageRuby},
		{"app.php", LanguagePHP},
		{"app.swift", LanguageSwift},
		{"app.kt", LanguageKotlin},
		{"unknown.xyz", LanguageUnknown},
	}

	for _, tt := range tests {
		got := LanguageFromExtension(tt.path)
		if got != tt.want {
			t.Errorf("LanguageFromExtension(%q) = %v, want %v", tt.path, got, tt.want)
		}
	}
}

func TestLanguage_Confidence(t *testing.T) {
	if LanguageUnknown.Confidence() != 0.5 {
		t.Errorf("LanguageUnknown.Confidence() = %v, want 0.5", LanguageUnknown.Confidence())
	}

	if LanguageGo.Confidence() != 0.95 {
		t.Errorf("LanguageGo.Confidence() = %v, want 0.95", LanguageGo.Confidence())
	}
}

func TestNewScore(t *testing.T) {
	score := NewScore()

	if score.StructuralComplexity != 20.0 {
		t.Errorf("StructuralComplexity = %v, want 20.0", score.StructuralComplexity)
	}
	if score.SemanticComplexity != 15.0 {
		t.Errorf("SemanticComplexity = %v, want 15.0", score.SemanticComplexity)
	}
	if score.Total != 100.0 {
		t.Errorf("Total = %v, want 100.0", score.Total)
	}
	if score.Grade != GradeAPlus {
		t.Errorf("Grade = %v, want A+", score.Grade)
	}
}

func TestScore_CalculateTotal(t *testing.T) {
	t.Run("normal calculation", func(t *testing.T) {
		score := NewScore()
		score.StructuralComplexity = 15.0
		score.SemanticComplexity = 10.0
		score.DuplicationRatio = 10.0
		score.CouplingScore = 10.0
		score.DocCoverage = 3.0
		score.ConsistencyScore = 8.0
		score.HotspotScore = 8.0
		score.TemporalCouplingScore = 8.0

		score.CalculateTotal()

		// 15+10+10+10+3+8+8+8 = 72
		if score.Total != 72.0 {
			t.Errorf("Total = %v, want 72.0", score.Total)
		}
		if score.Grade != GradeBMinus {
			t.Errorf("Grade = %v, want B-", score.Grade)
		}
	})

	t.Run("critical defects", func(t *testing.T) {
		score := NewScore()
		score.HasCriticalDefects = true

		score.CalculateTotal()

		if score.Total != 0.0 {
			t.Errorf("Total = %v, want 0.0 with critical defects", score.Total)
		}
		if score.Grade != GradeF {
			t.Errorf("Grade = %v, want F with critical defects", score.Grade)
		}
	})
}

func TestScore_SetMetric(t *testing.T) {
	score := NewScore()

	score.SetMetric(MetricStructuralComplexity, 10.0)
	if score.StructuralComplexity != 10.0 {
		t.Errorf("StructuralComplexity = %v, want 10.0", score.StructuralComplexity)
	}

	score.SetMetric(MetricDocumentation, 3.0)
	if score.DocCoverage != 3.0 {
		t.Errorf("DocCoverage = %v, want 3.0", score.DocCoverage)
	}
}

func TestPenaltyTracker(t *testing.T) {
	tracker := NewPenaltyTracker()

	// First application should succeed
	applied := tracker.Apply("issue1", MetricStructuralComplexity, 5.0, "Test issue")
	if applied != 5.0 {
		t.Errorf("First Apply() = %v, want 5.0", applied)
	}

	// Second application of same issue should return 0
	applied = tracker.Apply("issue1", MetricStructuralComplexity, 5.0, "Test issue")
	if applied != 0.0 {
		t.Errorf("Second Apply() = %v, want 0.0", applied)
	}

	// Different issue should succeed
	applied = tracker.Apply("issue2", MetricSemanticComplexity, 3.0, "Another issue")
	if applied != 3.0 {
		t.Errorf("Different issue Apply() = %v, want 3.0", applied)
	}

	attrs := tracker.GetAttributions()
	if len(attrs) != 2 {
		t.Errorf("GetAttributions() length = %v, want 2", len(attrs))
	}
}

func TestAggregateProjectScore(t *testing.T) {
	scores := []Score{
		{Total: 80, Grade: GradeBPlus, Language: LanguageGo},
		{Total: 90, Grade: GradeA, Language: LanguageGo},
		{Total: 70, Grade: GradeBMinus, Language: LanguageRust},
	}

	project := AggregateProjectScore(scores)

	if project.TotalFiles != 3 {
		t.Errorf("TotalFiles = %v, want 3", project.TotalFiles)
	}

	expectedAvg := float32(80.0) // (80+90+70)/3 = 80
	if project.AverageScore != expectedAvg {
		t.Errorf("AverageScore = %v, want %v", project.AverageScore, expectedAvg)
	}

	if project.LanguageDistribution[LanguageGo] != 2 {
		t.Errorf("Go count = %v, want 2", project.LanguageDistribution[LanguageGo])
	}

	if project.LanguageDistribution[LanguageRust] != 1 {
		t.Errorf("Rust count = %v, want 1", project.LanguageDistribution[LanguageRust])
	}
}

func TestProjectScore_Average(t *testing.T) {
	t.Run("with files", func(t *testing.T) {
		project := ProjectScore{
			Files: []Score{
				{StructuralComplexity: 18, SemanticComplexity: 12, DuplicationRatio: 14, CouplingScore: 13, DocCoverage: 4, ConsistencyScore: 9, Confidence: 0.9},
				{StructuralComplexity: 16, SemanticComplexity: 10, DuplicationRatio: 12, CouplingScore: 11, DocCoverage: 3, ConsistencyScore: 8, Confidence: 0.8},
			},
		}

		avg := project.Average()

		if avg.StructuralComplexity != 17.0 {
			t.Errorf("StructuralComplexity = %v, want 17.0", avg.StructuralComplexity)
		}
		if avg.Confidence != 0.85 {
			t.Errorf("Confidence = %v, want 0.85", avg.Confidence)
		}
	})

	t.Run("empty files", func(t *testing.T) {
		project := ProjectScore{Files: nil}
		avg := project.Average()

		if avg.Total != 100.0 {
			t.Errorf("Empty project Average().Total = %v, want 100.0", avg.Total)
		}
	})
}

func TestNewComparison(t *testing.T) {
	score1 := Score{
		Total:                80,
		FilePath:             "file1.go",
		StructuralComplexity: 15,
		SemanticComplexity:   10,
		DuplicationRatio:     12,
		DocCoverage:          3,
	}
	score2 := Score{
		Total:                90,
		FilePath:             "file2.go",
		StructuralComplexity: 18,
		SemanticComplexity:   12,
		DuplicationRatio:     10,
		DocCoverage:          4,
	}

	comp := NewComparison(score1, score2)

	if comp.Delta != 10.0 {
		t.Errorf("Delta = %v, want 10.0", comp.Delta)
	}

	if comp.Winner != "file2.go" {
		t.Errorf("Winner = %v, want file2.go", comp.Winner)
	}

	// Score2 is better in structural, semantic, and doc coverage
	if len(comp.Improvements) < 3 {
		t.Errorf("Improvements count = %v, want >= 3", len(comp.Improvements))
	}

	// Score2 is worse in duplication
	if len(comp.Regressions) < 1 {
		t.Errorf("Regressions count = %v, want >= 1", len(comp.Regressions))
	}
}

func TestSeverityFromValue(t *testing.T) {
	tests := []struct {
		value float64
		want  Severity
	}{
		{0.5, SeverityNormal},
		{1.5, SeverityNormal},
		{1.6, SeverityWarning},
		{2.5, SeverityWarning},
		{2.6, SeverityCritical},
		{5.0, SeverityCritical},
	}

	for _, tt := range tests {
		got := SeverityFromValue(tt.value)
		if got != tt.want {
			t.Errorf("SeverityFromValue(%v) = %v, want %v", tt.value, got, tt.want)
		}
	}
}

func TestProjectScore_ToReport(t *testing.T) {
	project := &ProjectScore{
		TotalFiles: 3,
		Files: []Score{
			{Total: 90, FilePath: "good.go", StructuralComplexity: 18, SemanticComplexity: 13, CouplingScore: 14, DuplicationRatio: 14},
			{Total: 60, FilePath: "medium.go", StructuralComplexity: 12, SemanticComplexity: 8, CouplingScore: 10, DuplicationRatio: 10},
			{Total: 30, FilePath: "bad.go", StructuralComplexity: 5, SemanticComplexity: 4, CouplingScore: 5, DuplicationRatio: 5},
		},
	}

	report := project.ToReport(10)

	if report.Summary.TotalFiles != 3 {
		t.Errorf("TotalFiles = %v, want 3", report.Summary.TotalFiles)
	}

	if len(report.Hotspots) != 3 {
		t.Errorf("Hotspots count = %v, want 3", len(report.Hotspots))
	}

	// Hotspots should be sorted by TDG score descending (worst first)
	if report.Hotspots[0].Path != "bad.go" {
		t.Errorf("First hotspot = %v, want bad.go", report.Hotspots[0].Path)
	}

	if report.Summary.CriticalFiles < 1 {
		t.Errorf("CriticalFiles = %v, want >= 1", report.Summary.CriticalFiles)
	}
}

func TestIsDocComment(t *testing.T) {
	analyzer := New()

	tests := []struct {
		line     string
		language Language
		want     bool
	}{
		{"/// This is a doc comment", LanguageRust, true},
		{"//! Module doc", LanguageRust, true},
		{"// regular comment", LanguageRust, false},
		{`"""docstring"""`, LanguagePython, true},
		{"// Go doc comment", LanguageGo, true},
		{"/** JSDoc comment", LanguageJavaScript, true},
		{"* continued jsdoc", LanguageTypeScript, true},
	}

	for _, tt := range tests {
		got := analyzer.isDocComment(tt.line, tt.language)
		if got != tt.want {
			t.Errorf("isDocComment(%q, %v) = %v, want %v", tt.line, tt.language, got, tt.want)
		}
	}
}

func TestEstimateDuplicationRatio(t *testing.T) {
	analyzer := New()

	t.Run("no duplication", func(t *testing.T) {
		source := `func a() { println("a") }
func b() { println("b") }
func c() { println("c") }`

		ratio := analyzer.estimateDuplicationRatio(source)
		if ratio > 0.1 {
			t.Errorf("ratio = %v, want < 0.1 for unique lines", ratio)
		}
	})

	t.Run("with duplication", func(t *testing.T) {
		source := `processDataWithLongFunctionName()
processDataWithLongFunctionName()
processDataWithLongFunctionName()
processDataWithLongFunctionName()
processDataWithLongFunctionName()
uniqueLine()`

		ratio := analyzer.estimateDuplicationRatio(source)
		if ratio < 0.5 {
			t.Errorf("ratio = %v, want >= 0.5 for duplicated lines", ratio)
		}
	})

	t.Run("short source", func(t *testing.T) {
		source := `a
b`
		ratio := analyzer.estimateDuplicationRatio(source)
		if ratio != 0 {
			t.Errorf("ratio = %v, want 0 for short source", ratio)
		}
	})
}
