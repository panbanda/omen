package analyzer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/models"
)

func TestTdgAnalyzerAnalyzeSource(t *testing.T) {
	analyzer := NewTdgAnalyzer()
	defer analyzer.Close()

	source := `
// A simple function
func simple() int {
    return 42
}
`

	score, err := analyzer.AnalyzeSource(source, models.LanguageGo, "test.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	if score.Language != models.LanguageGo {
		t.Errorf("Language = %v, want Go", score.Language)
	}

	if score.Total <= 0 || score.Total > 100 {
		t.Errorf("Total = %v, want 0-100", score.Total)
	}

	if score.Confidence <= 0 || score.Confidence > 1 {
		t.Errorf("Confidence = %v, want 0-1", score.Confidence)
	}
}

func TestTdgAnalyzerComplexCode(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

	score, err := analyzer.AnalyzeSource(source, models.LanguageGo, "complex.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// Deep nesting should result in penalties
	if score.SemanticComplexity >= 20.0 {
		t.Errorf("Deep nesting should reduce SemanticComplexity, got %v", score.SemanticComplexity)
	}

	if len(score.PenaltiesApplied) == 0 {
		t.Error("Complex code should have penalties applied")
	}
}

func TestTdgAnalyzerDuplicateCode(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

	score, err := analyzer.AnalyzeSource(source, models.LanguageGo, "dup.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// Duplication should be detected
	if score.DuplicationRatio == 20.0 {
		t.Error("Duplicated code should reduce duplication score")
	}
}

func TestTdgAnalyzerHighCoupling(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

	score, err := analyzer.AnalyzeSource(source, models.LanguageGo, "coupling.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// High coupling should reduce coupling score
	if score.CouplingScore >= 15.0 {
		t.Errorf("High coupling should reduce CouplingScore, got %v", score.CouplingScore)
	}
}

func TestTdgAnalyzerDocumentation(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

	score, err := analyzer.AnalyzeSource(source, models.LanguageGo, "doc.go")
	if err != nil {
		t.Fatalf("AnalyzeSource() error = %v", err)
	}

	// Good documentation should give points
	if score.DocCoverage <= 0 {
		t.Errorf("Well-documented code should have DocCoverage > 0, got %v", score.DocCoverage)
	}
}

func TestTdgAnalyzerCriticalDefects(t *testing.T) {
	analyzer := NewTdgAnalyzer()
	defer analyzer.Close()

	// Rust code with .unwrap()
	source := `
fn main() {
    let value = some_result().unwrap();
    let other = another_result().unwrap();
    panic!("something went wrong");
}
`

	score, err := analyzer.AnalyzeSource(source, models.LanguageRust, "defects.rs")
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
	if score.Grade != models.GradeF {
		t.Errorf("Critical defects should result in Grade F, got %v", score.Grade)
	}

	if score.Total != 0 {
		t.Errorf("Critical defects should result in Total 0, got %v", score.Total)
	}
}

func TestTdgAnalyzerAnalyzeFile(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

	if score.Language != models.LanguageGo {
		t.Errorf("Language = %v, want Go", score.Language)
	}
}

func TestTdgAnalyzerAnalyzeProject(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

	if project.LanguageDistribution[models.LanguageGo] != 3 {
		t.Errorf("Go files = %v, want 3", project.LanguageDistribution[models.LanguageGo])
	}
}

func TestTdgAnalyzerCompare(t *testing.T) {
	analyzer := NewTdgAnalyzer()
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

func TestTdgAnalyzerSkipDirectories(t *testing.T) {
	analyzer := NewTdgAnalyzer()

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

func TestTdgAnalyzerShouldAnalyzeFile(t *testing.T) {
	analyzer := NewTdgAnalyzer()

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

func TestTdgAnalyzerEstimateCyclomaticComplexity(t *testing.T) {
	analyzer := NewTdgAnalyzer()

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
			lines := splitLines(tc.source)
			got := analyzer.estimateCyclomaticComplexity(lines)
			if got < tc.minExpected {
				t.Errorf("estimateCyclomaticComplexity() = %v, want >= %v", got, tc.minExpected)
			}
		})
	}
}

func TestTdgAnalyzerEstimateNestingDepth(t *testing.T) {
	analyzer := NewTdgAnalyzer()

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

func splitLines(s string) []string {
	var lines []string
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == '\n' {
			lines = append(lines, s[start:i])
			start = i + 1
		}
	}
	if start < len(s) {
		lines = append(lines, s[start:])
	}
	return lines
}

func TestTdgAnalyzerWithOptions(t *testing.T) {
	t.Run("default_options", func(t *testing.T) {
		analyzer := NewTdgAnalyzer()
		defer analyzer.Close()

		if analyzer.maxFileSize != 0 {
			t.Errorf("maxFileSize = %v, want 0", analyzer.maxFileSize)
		}

		if analyzer.config.Weights.StructuralComplexity == 0 {
			t.Error("config should be set to defaults")
		}
	})

	t.Run("with_max_file_size", func(t *testing.T) {
		maxSize := int64(1024)
		analyzer := NewTdgAnalyzer(WithTdgMaxFileSize(maxSize))
		defer analyzer.Close()

		if analyzer.maxFileSize != maxSize {
			t.Errorf("maxFileSize = %v, want %v", analyzer.maxFileSize, maxSize)
		}
	})

	t.Run("with_custom_config", func(t *testing.T) {
		customConfig := models.DefaultTdgConfig()
		customConfig.Weights.StructuralComplexity = 999

		analyzer := NewTdgAnalyzer(WithTdgConfig(customConfig))
		defer analyzer.Close()

		if analyzer.config.Weights.StructuralComplexity != 999 {
			t.Errorf("StructuralComplexity = %v, want 999", analyzer.config.Weights.StructuralComplexity)
		}
	})

	t.Run("with_multiple_options", func(t *testing.T) {
		customConfig := models.DefaultTdgConfig()
		customConfig.Weights.StructuralComplexity = 999
		maxSize := int64(2048)

		analyzer := NewTdgAnalyzer(
			WithTdgConfig(customConfig),
			WithTdgMaxFileSize(maxSize),
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

func TestTdgAnalyzerMaxFileSize(t *testing.T) {
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
		analyzer := NewTdgAnalyzer()
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
		analyzer := NewTdgAnalyzer(WithTdgMaxFileSize(100))
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
