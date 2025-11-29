package analyzer

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/panbanda/omen/internal/scanner"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

func TestNewSATDAnalyzer(t *testing.T) {
	analyzer := NewSATDAnalyzer()
	if analyzer == nil {
		t.Fatal("NewSATDAnalyzer returned nil")
	}

	if len(analyzer.patterns) == 0 {
		t.Error("NewSATDAnalyzer should initialize with default patterns")
	}

	expectedPatterns := 21
	if len(analyzer.patterns) != expectedPatterns {
		t.Errorf("Expected %d default patterns, got %d", expectedPatterns, len(analyzer.patterns))
	}
}

func TestAddPattern(t *testing.T) {
	tests := []struct {
		name        string
		pattern     string
		category    models.DebtCategory
		severity    models.Severity
		expectError bool
	}{
		{
			name:        "valid pattern",
			pattern:     `(?i)\bCUSTOM\b[:\s]*(.+)?`,
			category:    models.DebtDesign,
			severity:    models.SeverityMedium,
			expectError: false,
		},
		{
			name:        "invalid regex",
			pattern:     `[unclosed`,
			category:    models.DebtDefect,
			severity:    models.SeverityHigh,
			expectError: true,
		},
		{
			name:        "simple word pattern",
			pattern:     `DEPRECATED`,
			category:    models.DebtDesign,
			severity:    models.SeverityLow,
			expectError: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewSATDAnalyzer()
			initialCount := len(analyzer.patterns)

			err := analyzer.AddPattern(tt.pattern, tt.category, tt.severity)

			if tt.expectError {
				if err == nil {
					t.Error("Expected error for invalid pattern, got nil")
				}
				if len(analyzer.patterns) != initialCount {
					t.Error("Pattern count should not change on error")
				}
			} else {
				if err != nil {
					t.Errorf("Unexpected error: %v", err)
				}
				if len(analyzer.patterns) != initialCount+1 {
					t.Errorf("Expected %d patterns, got %d", initialCount+1, len(analyzer.patterns))
				}
			}
		})
	}
}

func TestAnalyzeFile_AllSeverityLevels(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		severity models.Severity
		category models.DebtCategory
		marker   string
	}{
		{
			name:     "critical security",
			content:  "// SECURITY: potential XSS vulnerability\n",
			severity: models.SeverityCritical,
			category: models.DebtSecurity,
			marker:   "SECURITY",
		},
		{
			name:     "critical unsafe",
			content:  "// UNSAFE: pointer manipulation\n",
			severity: models.SeverityCritical,
			category: models.DebtSecurity,
			marker:   "UNKNOWN",
		},
		{
			name:     "high fixme",
			content:  "// FIXME: broken logic here\n",
			severity: models.SeverityHigh,
			category: models.DebtDefect,
			marker:   "FIXME",
		},
		{
			name:     "high bug",
			content:  "// BUG: race condition\n",
			severity: models.SeverityHigh,
			category: models.DebtDefect,
			marker:   "BUG",
		},
		{
			name:     "medium hack",
			content:  "// HACK: temporary workaround\n",
			severity: models.SeverityMedium,
			category: models.DebtDesign,
			marker:   "HACK",
		},
		{
			name:     "medium refactor",
			content:  "// REFACTOR: extract method\n",
			severity: models.SeverityMedium,
			category: models.DebtDesign,
			marker:   "REFACTOR",
		},
		{
			name:     "low todo",
			content:  "// TODO: add validation\n",
			severity: models.SeverityLow,
			category: models.DebtRequirement,
			marker:   "TODO",
		},
		{
			name:     "low optimize",
			content:  "// OPTIMIZE: use binary search\n",
			severity: models.SeverityLow,
			category: models.DebtPerformance,
			marker:   "OPTIMIZE",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != 1 {
				t.Fatalf("Expected 1 debt item, got %d", len(debts))
			}

			debt := debts[0]
			if debt.Severity != tt.severity {
				t.Errorf("Expected severity %s, got %s", tt.severity, debt.Severity)
			}
			if debt.Category != tt.category {
				t.Errorf("Expected category %s, got %s", tt.category, debt.Category)
			}
			if debt.Marker != tt.marker {
				t.Errorf("Expected marker %s, got %s", tt.marker, debt.Marker)
			}
			if debt.Line != 1 {
				t.Errorf("Expected line 1, got %d", debt.Line)
			}
		})
	}
}

func TestAnalyzeFile_AllCategories(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		category models.DebtCategory
	}{
		{
			name:     "security category",
			content:  "// VULNERABILITY: SQL injection possible\n",
			category: models.DebtSecurity,
		},
		{
			name:     "defect category",
			content:  "// BROKEN: function returns wrong value\n",
			category: models.DebtDefect,
		},
		{
			name:     "design category",
			content:  "// KLUDGE: need to redesign this\n",
			category: models.DebtDesign,
		},
		{
			name:     "requirement category",
			content:  "// TODO: implement feature X\n",
			category: models.DebtRequirement,
		},
		{
			name:     "test category",
			content:  "// TEST: needs unit tests\n",
			category: models.DebtTest,
		},
		{
			name:     "performance category",
			content:  "// SLOW: O(n^2) algorithm\n",
			category: models.DebtPerformance,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != 1 {
				t.Fatalf("Expected 1 debt item, got %d", len(debts))
			}

			if debts[0].Category != tt.category {
				t.Errorf("Expected category %s, got %s", tt.category, debts[0].Category)
			}
		})
	}
}

func TestAnalyzeFile_VariousMarkers(t *testing.T) {
	tests := []struct {
		name          string
		content       string
		expectedCount int
		markers       []string
	}{
		{
			name:          "multiple markers",
			content:       "// TODO: fix this\n// FIXME: broken\n// HACK: temporary\n",
			expectedCount: 3,
			markers:       []string{"TODO", "FIXME", "HACK"},
		},
		{
			name:          "case insensitive",
			content:       "// todo: lowercase\n// Todo: mixed case\n// TODO: uppercase\n",
			expectedCount: 3,
			markers:       []string{"TODO", "TODO", "TODO"},
		},
		{
			name:          "with colons",
			content:       "// TODO: with colon\n// TODO without colon\n",
			expectedCount: 2,
			markers:       []string{"TODO", "TODO"},
		},
		{
			name:          "xxx marker",
			content:       "// XXX: danger zone\n",
			expectedCount: 1,
			markers:       []string{"XXX"},
		},
		{
			name:          "note marker",
			content:       "// NOTE: important detail\n",
			expectedCount: 1,
			markers:       []string{"NOTE"},
		},
		{
			name:          "cleanup marker",
			content:       "// CLEANUP: remove old code\n",
			expectedCount: 1,
			markers:       []string{"CLEANUP"},
		},
		{
			name:          "technical debt phrase",
			content:       "// This is technical debt\n",
			expectedCount: 1,
			markers:       []string{"UNKNOWN"},
		},
		{
			name:          "code smell phrase",
			content:       "// This code smell needs fixing\n",
			expectedCount: 1,
			markers:       []string{"UNKNOWN"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != tt.expectedCount {
				t.Fatalf("Expected %d debt items, got %d", tt.expectedCount, len(debts))
			}

			for i, expectedMarker := range tt.markers {
				if debts[i].Marker != expectedMarker {
					t.Errorf("Item %d: expected marker %s, got %s", i, expectedMarker, debts[i].Marker)
				}
			}
		})
	}
}

func TestGetCommentStyle(t *testing.T) {
	tests := []struct {
		name               string
		lang               parser.Language
		expectedLine       []string
		expectedBlockStart string
		expectedBlockEnd   string
	}{
		{
			name:               "go language",
			lang:               parser.LangGo,
			expectedLine:       []string{"//"},
			expectedBlockStart: "/*",
			expectedBlockEnd:   "*/",
		},
		{
			name:               "python language",
			lang:               parser.LangPython,
			expectedLine:       []string{"#"},
			expectedBlockStart: `"""`,
			expectedBlockEnd:   `"""`,
		},
		{
			name:               "ruby language",
			lang:               parser.LangRuby,
			expectedLine:       []string{"#"},
			expectedBlockStart: `"""`,
			expectedBlockEnd:   `"""`,
		},
		{
			name:               "bash language",
			lang:               parser.LangBash,
			expectedLine:       []string{"#"},
			expectedBlockStart: `"""`,
			expectedBlockEnd:   `"""`,
		},
		{
			name:               "rust language",
			lang:               parser.LangRust,
			expectedLine:       []string{"//"},
			expectedBlockStart: "/*",
			expectedBlockEnd:   "*/",
		},
		{
			name:               "java language",
			lang:               parser.LangJava,
			expectedLine:       []string{"//"},
			expectedBlockStart: "/*",
			expectedBlockEnd:   "*/",
		},
		{
			name:               "typescript language",
			lang:               parser.LangTypeScript,
			expectedLine:       []string{"//"},
			expectedBlockStart: "/*",
			expectedBlockEnd:   "*/",
		},
		{
			name:               "unknown language",
			lang:               parser.LangUnknown,
			expectedLine:       []string{"//", "#"},
			expectedBlockStart: "/*",
			expectedBlockEnd:   "*/",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			style := getCommentStyle(tt.lang)

			if len(style.lineComments) != len(tt.expectedLine) {
				t.Fatalf("Expected %d line comment styles, got %d", len(tt.expectedLine), len(style.lineComments))
			}

			for i, expected := range tt.expectedLine {
				if style.lineComments[i] != expected {
					t.Errorf("Expected line comment '%s', got '%s'", expected, style.lineComments[i])
				}
			}

			if style.blockStart != tt.expectedBlockStart {
				t.Errorf("Expected block start '%s', got '%s'", tt.expectedBlockStart, style.blockStart)
			}

			if style.blockEnd != tt.expectedBlockEnd {
				t.Errorf("Expected block end '%s', got '%s'", tt.expectedBlockEnd, style.blockEnd)
			}
		})
	}
}

func TestIsCommentLine(t *testing.T) {
	tests := []struct {
		name     string
		line     string
		style    commentStyle
		expected bool
	}{
		{
			name:     "single line comment with //",
			line:     "// This is a comment",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: true,
		},
		{
			name:     "single line comment with #",
			line:     "# This is a comment",
			style:    commentStyle{lineComments: []string{"#"}, blockStart: `"""`, blockEnd: `"""`},
			expected: true,
		},
		{
			name:     "block comment start",
			line:     "/* Block comment",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: true,
		},
		{
			name:     "block comment end",
			line:     "end of comment */",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: true,
		},
		{
			name:     "block comment middle with asterisk",
			line:     " * Middle of block comment",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: true,
		},
		{
			name:     "regular code",
			line:     "func main() {",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: false,
		},
		{
			name:     "code with comment marker in string",
			line:     `message := "// not a comment"`,
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: false,
		},
		{
			name:     "indented comment",
			line:     "    // Indented comment",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: true,
		},
		{
			name:     "python docstring",
			line:     `"""Docstring"""`,
			style:    commentStyle{lineComments: []string{"#"}, blockStart: `"""`, blockEnd: `"""`},
			expected: true,
		},
		{
			name:     "empty line",
			line:     "",
			style:    commentStyle{lineComments: []string{"//"}, blockStart: "/*", blockEnd: "*/"},
			expected: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := isCommentLine(tt.line, tt.style)
			if result != tt.expected {
				t.Errorf("Expected %v for line '%s', got %v", tt.expected, tt.line, result)
			}
		})
	}
}

func TestExtractMarker(t *testing.T) {
	tests := []struct {
		name     string
		match    string
		expected string
	}{
		{
			name:     "TODO marker",
			match:    "TODO: fix this",
			expected: "TODO",
		},
		{
			name:     "FIXME marker",
			match:    "FIXME: broken",
			expected: "FIXME",
		},
		{
			name:     "HACK marker",
			match:    "HACK: temporary solution",
			expected: "HACK",
		},
		{
			name:     "BUG marker",
			match:    "BUG: race condition",
			expected: "BUG",
		},
		{
			name:     "XXX marker",
			match:    "XXX: danger",
			expected: "XXX",
		},
		{
			name:     "NOTE marker",
			match:    "NOTE: important",
			expected: "NOTE",
		},
		{
			name:     "OPTIMIZE marker",
			match:    "OPTIMIZE: slow code",
			expected: "OPTIMIZE",
		},
		{
			name:     "REFACTOR marker",
			match:    "REFACTOR: extract method",
			expected: "REFACTOR",
		},
		{
			name:     "CLEANUP marker",
			match:    "CLEANUP: remove old code",
			expected: "CLEANUP",
		},
		{
			name:     "TEMP marker",
			match:    "TEMP: temporary fix",
			expected: "TEMP",
		},
		{
			name:     "WORKAROUND marker",
			match:    "WORKAROUND: API limitation",
			expected: "WORKAROUND",
		},
		{
			name:     "SECURITY marker",
			match:    "SECURITY: check input",
			expected: "SECURITY",
		},
		{
			name:     "TEST marker",
			match:    "TEST: needs coverage",
			expected: "TEST",
		},
		{
			name:     "lowercase todo",
			match:    "todo: lowercase",
			expected: "TODO",
		},
		{
			name:     "unknown marker",
			match:    "CUSTOM: not recognized",
			expected: "UNKNOWN",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := extractMarker(tt.match)
			if result != tt.expected {
				t.Errorf("Expected marker '%s', got '%s'", tt.expected, result)
			}
		})
	}
}

func TestAnalyzeFile_DifferentLanguages(t *testing.T) {
	tests := []struct {
		name     string
		filename string
		content  string
		expected int
	}{
		{
			name:     "go file",
			filename: "test.go",
			content:  "// TODO: implement\nfunc main() {}\n",
			expected: 1,
		},
		{
			name:     "python file",
			filename: "test.py",
			content:  "# TODO: implement\ndef main():\n    pass\n",
			expected: 1,
		},
		{
			name:     "rust file",
			filename: "test.rs",
			content:  "// TODO: implement\nfn main() {}\n",
			expected: 1,
		},
		{
			name:     "javascript file",
			filename: "test.js",
			content:  "// TODO: implement\nfunction main() {}\n",
			expected: 1,
		},
		{
			name:     "typescript file",
			filename: "test.ts",
			content:  "// TODO: implement\nfunction main(): void {}\n",
			expected: 1,
		},
		{
			name:     "java file",
			filename: "Test.java",
			content:  "// TODO: implement\npublic class Test {}\n",
			expected: 1,
		},
		{
			name:     "ruby file",
			filename: "test.rb",
			content:  "# TODO: implement\ndef main\nend\n",
			expected: 1,
		},
		{
			name:     "bash file",
			filename: "test.sh",
			content:  "# TODO: implement\necho 'test'\n",
			expected: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, tt.filename)
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != tt.expected {
				t.Errorf("Expected %d debt items, got %d", tt.expected, len(debts))
			}
		})
	}
}

func TestSATDAnalyzeFile_EdgeCases(t *testing.T) {
	tests := []struct {
		name          string
		content       string
		expectedCount int
	}{
		{
			name:          "empty file",
			content:       "",
			expectedCount: 0,
		},
		{
			name:          "no comments",
			content:       "func main() {\n    println('hello')\n}\n",
			expectedCount: 0,
		},
		{
			name:          "no SATD markers",
			content:       "// This is a normal comment\n// Just explaining the code\n",
			expectedCount: 0,
		},
		{
			name:          "markers in code not comments",
			content:       `message := "TODO: this is in a string"\n`,
			expectedCount: 0,
		},
		{
			name:          "multiple markers same line",
			content:       "// TODO: FIXME: multiple markers\n",
			expectedCount: 1,
		},
		{
			name: "block comment with markers",
			content: `/*
 * TODO: implement feature
 * FIXME: broken logic
 */
`,
			expectedCount: 2,
		},
		{
			name:          "very long line",
			content:       "// TODO: " + string(make([]byte, 10000)) + "\n",
			expectedCount: 1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != tt.expectedCount {
				t.Errorf("Expected %d debt items, got %d", tt.expectedCount, len(debts))
			}
		})
	}
}

func TestAnalyzeFile_LineNumbers(t *testing.T) {
	content := `package main

// TODO: add validation
func validate() {}

// FIXME: handle errors
func process() {
    // HACK: temporary workaround
    doSomething()
}
`

	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)

	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	expectedLines := []uint32{3, 6, 8}
	if len(debts) != len(expectedLines) {
		t.Fatalf("Expected %d debt items, got %d", len(expectedLines), len(debts))
	}

	for i, expectedLine := range expectedLines {
		if debts[i].Line != expectedLine {
			t.Errorf("Debt %d: expected line %d, got %d", i, expectedLine, debts[i].Line)
		}
	}
}

func TestAnalyzeFile_NonExistentFile(t *testing.T) {
	analyzer := NewSATDAnalyzer()
	_, err := analyzer.AnalyzeFile("/nonexistent/file.go")

	if err == nil {
		t.Error("Expected error for non-existent file, got nil")
	}
}

func TestSATDAnalyzeProject(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "file1.go")
	content1 := "// TODO: implement\n// FIXME: broken\n"
	if err := os.WriteFile(file1, []byte(content1), 0644); err != nil {
		t.Fatal(err)
	}

	file2 := filepath.Join(tmpDir, "file2.go")
	content2 := "// HACK: temporary\nfunc main() {}\n"
	if err := os.WriteFile(file2, []byte(content2), 0644); err != nil {
		t.Fatal(err)
	}

	file3 := filepath.Join(tmpDir, "file3.go")
	content3 := "// No SATD markers here\nfunc test() {}\n"
	if err := os.WriteFile(file3, []byte(content3), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	analysis, err := analyzer.AnalyzeProject([]string{file1, file2, file3})

	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.Summary.TotalItems != 3 {
		t.Errorf("Expected 3 total items, got %d", analysis.Summary.TotalItems)
	}

	if analysis.Summary.BySeverity[string(models.SeverityLow)] != 1 {
		t.Errorf("Expected 1 low severity item, got %d", analysis.Summary.BySeverity[string(models.SeverityLow)])
	}

	if analysis.Summary.BySeverity[string(models.SeverityHigh)] != 1 {
		t.Errorf("Expected 1 high severity item, got %d", analysis.Summary.BySeverity[string(models.SeverityHigh)])
	}

	if analysis.Summary.BySeverity[string(models.SeverityMedium)] != 1 {
		t.Errorf("Expected 1 medium severity item, got %d", analysis.Summary.BySeverity[string(models.SeverityMedium)])
	}

	if analysis.Summary.ByCategory[string(models.DebtRequirement)] != 1 {
		t.Errorf("Expected 1 requirement category item, got %d", analysis.Summary.ByCategory[string(models.DebtRequirement)])
	}

	if analysis.Summary.ByCategory[string(models.DebtDefect)] != 1 {
		t.Errorf("Expected 1 defect category item, got %d", analysis.Summary.ByCategory[string(models.DebtDefect)])
	}

	if analysis.Summary.ByCategory[string(models.DebtDesign)] != 1 {
		t.Errorf("Expected 1 design category item, got %d", analysis.Summary.ByCategory[string(models.DebtDesign)])
	}

	expectedFiles := 2
	if analysis.Summary.ByFile[file1] != 2 {
		t.Errorf("Expected 2 items in file1, got %d", analysis.Summary.ByFile[file1])
	}
	if analysis.Summary.ByFile[file2] != 1 {
		t.Errorf("Expected 1 item in file2, got %d", analysis.Summary.ByFile[file2])
	}
	if len(analysis.Summary.ByFile) != expectedFiles {
		t.Errorf("Expected %d files with debt, got %d", expectedFiles, len(analysis.Summary.ByFile))
	}
}

func TestSATDAnalyzeProject_EmptyFileList(t *testing.T) {
	analyzer := NewSATDAnalyzer()
	analysis, err := analyzer.AnalyzeProject([]string{})

	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.Summary.TotalItems != 0 {
		t.Errorf("Expected 0 items for empty file list, got %d", analysis.Summary.TotalItems)
	}
}

func TestSATDAnalyzeProject_WithProgress(t *testing.T) {
	tmpDir := t.TempDir()

	files := make([]string, 5)
	for i := 0; i < 5; i++ {
		files[i] = filepath.Join(tmpDir, "file"+string('0'+rune(i))+".go")
		content := "// TODO: implement\n"
		if err := os.WriteFile(files[i], []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}

	analyzer := NewSATDAnalyzer()
	var progressCount atomic.Int32
	progressFunc := func() {
		progressCount.Add(1)
	}

	analysis, err := analyzer.AnalyzeProjectWithProgress(files, progressFunc)

	if err != nil {
		t.Fatalf("AnalyzeProjectWithProgress failed: %v", err)
	}

	if analysis.Summary.TotalItems != 5 {
		t.Errorf("Expected 5 items, got %d", analysis.Summary.TotalItems)
	}

	if progressCount.Load() != 5 {
		t.Errorf("Expected progress callback to be called 5 times, got %d", progressCount.Load())
	}
}

func TestAnalyzeFile_SpecialPatterns(t *testing.T) {
	tests := []struct {
		name          string
		content       string
		expectedCount int
		marker        string
		severity      models.Severity
		category      models.DebtCategory
	}{
		{
			name:          "CVE mention",
			content:       "// CVE-2023-1234: security issue\n",
			expectedCount: 1,
			marker:        "SECURITY",
			severity:      models.SeverityCritical,
			category:      models.DebtSecurity,
		},
		{
			name:          "performance issue phrase",
			content:       "// This is a performance issue\n",
			expectedCount: 1,
			marker:        "UNKNOWN",
			severity:      models.SeverityMedium,
			category:      models.DebtPerformance,
		},
		{
			name:          "test disabled",
			content:       "// test disabled due to flakiness\n",
			expectedCount: 1,
			marker:        "TEST",
			severity:      models.SeverityMedium,
			category:      models.DebtTest,
		},
		{
			name:          "UNTESTED marker",
			content:       "// UNTESTED: needs test coverage\n",
			expectedCount: 1,
			marker:        "TEST",
			severity:      models.SeverityLow,
			category:      models.DebtTest,
		},
		{
			name:          "FIX ME with space",
			content:       "// FIX ME: broken logic\n",
			expectedCount: 1,
			marker:        "UNKNOWN",
			severity:      models.SeverityHigh,
			category:      models.DebtDefect,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "regular.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			// Use no severity adjustment to test raw pattern matching
			analyzer := NewSATDAnalyzer(
				WithSATDIncludeTests(true),
				WithSATDAdjustSeverity(false),
			)
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != tt.expectedCount {
				t.Fatalf("Expected %d debt items, got %d", tt.expectedCount, len(debts))
			}

			if tt.expectedCount > 0 {
				debt := debts[0]
				if debt.Marker != tt.marker {
					t.Errorf("Expected marker %s, got %s", tt.marker, debt.Marker)
				}
				if debt.Severity != tt.severity {
					t.Errorf("Expected severity %s, got %s", tt.severity, debt.Severity)
				}
				if debt.Category != tt.category {
					t.Errorf("Expected category %s, got %s", tt.category, debt.Category)
				}
			}
		})
	}
}

func TestNewSATDAnalyzerWithOptions(t *testing.T) {
	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(false),
		WithSATDIncludeVendor(false),
		WithSATDAdjustSeverity(true),
	)
	if analyzer == nil {
		t.Fatal("NewSATDAnalyzer returned nil")
	}

	if analyzer.includeTests {
		t.Error("IncludeTests should be false")
	}
	if analyzer.includeVendor {
		t.Error("IncludeVendor should be false")
	}
	if !analyzer.adjustSeverity {
		t.Error("AdjustSeverity should be true")
	}
	if !analyzer.generateContextID {
		t.Error("GenerateContextID should be true")
	}
}

func TestDefaultSATDOptions(t *testing.T) {
	analyzer := NewSATDAnalyzer()

	if !analyzer.includeTests {
		t.Error("IncludeTests should default to true")
	}
	if analyzer.includeVendor {
		t.Error("IncludeVendor should default to false")
	}
	if !analyzer.adjustSeverity {
		t.Error("AdjustSeverity should default to true")
	}
	if !analyzer.generateContextID {
		t.Error("GenerateContextID should default to true")
	}
}

func TestSATD_TestFileExclusion(t *testing.T) {
	tmpDir := t.TempDir()

	testFile := filepath.Join(tmpDir, "example_test.go")
	content := "// TODO: implement test\n"
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// With test exclusion disabled
	analyzerWithTests := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
	)
	debtsWithTests, err := analyzerWithTests.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debtsWithTests) != 1 {
		t.Errorf("Expected 1 debt item when including tests, got %d", len(debtsWithTests))
	}

	// With test exclusion enabled
	analyzerNoTests := NewSATDAnalyzer(
		WithSATDIncludeTests(false),
		WithSATDAdjustSeverity(false),
	)
	debtsNoTests, err := analyzerNoTests.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debtsNoTests) != 0 {
		t.Errorf("Expected 0 debt items when excluding tests, got %d", len(debtsNoTests))
	}
}

func TestSATD_SeverityAdjustment(t *testing.T) {
	tmpDir := t.TempDir()

	// Test file should reduce severity
	testFile := filepath.Join(tmpDir, "auth_test.go")
	content := "// FIXME: broken logic\n"
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(true),
	)
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 1 {
		t.Fatalf("Expected 1 debt item, got %d", len(debts))
	}

	// FIXME is normally High, test file reduces to Medium
	if debts[0].Severity != models.SeverityMedium {
		t.Errorf("Expected severity Medium (reduced from High) for test file, got %s", debts[0].Severity)
	}

	// Security file should escalate severity
	securityFile := filepath.Join(tmpDir, "auth_handler.go")
	content2 := "// TODO: add validation\n"
	if err := os.WriteFile(securityFile, []byte(content2), 0644); err != nil {
		t.Fatal(err)
	}

	debts2, err := analyzer.AnalyzeFile(securityFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts2) != 1 {
		t.Fatalf("Expected 1 debt item, got %d", len(debts2))
	}

	// TODO is normally Low, auth file escalates to Medium
	if debts2[0].Severity != models.SeverityMedium {
		t.Errorf("Expected severity Medium (escalated from Low) for security file, got %s", debts2[0].Severity)
	}
}

func TestSATD_ContextHashGeneration(t *testing.T) {
	tmpDir := t.TempDir()

	testFile := filepath.Join(tmpDir, "example.go")
	content := "// TODO: implement feature\n"
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// With context hash
	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
	)
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 1 {
		t.Fatalf("Expected 1 debt item, got %d", len(debts))
	}
	if debts[0].ContextHash == "" {
		t.Error("Expected non-empty context hash")
	}
	if len(debts[0].ContextHash) != 32 {
		t.Errorf("Expected 32 char hex hash (16 bytes), got %d chars", len(debts[0].ContextHash))
	}

	// Without context hash
	analyzerNoHash := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
		WithSATDGenerateContextID(false),
	)
	debtsNoHash, err := analyzerNoHash.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debtsNoHash) != 1 {
		t.Fatalf("Expected 1 debt item, got %d", len(debtsNoHash))
	}
	if debtsNoHash[0].ContextHash != "" {
		t.Error("Expected empty context hash when disabled")
	}
}

func TestSATD_VendorExclusion(t *testing.T) {
	tmpDir := t.TempDir()

	// Create vendor directory
	vendorDir := filepath.Join(tmpDir, "vendor", "lib")
	if err := os.MkdirAll(vendorDir, 0755); err != nil {
		t.Fatal(err)
	}

	vendorFile := filepath.Join(vendorDir, "lib.go")
	content := "// TODO: vendor todo\n"
	if err := os.WriteFile(vendorFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// With vendor exclusion (default)
	analyzer := NewSATDAnalyzer(
		WithSATDIncludeVendor(false),
		WithSATDAdjustSeverity(false),
	)
	debts, err := analyzer.AnalyzeFile(vendorFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 0 {
		t.Errorf("Expected 0 debt items for vendor file, got %d", len(debts))
	}

	// With vendor inclusion
	analyzerWithVendor := NewSATDAnalyzer(
		WithSATDIncludeVendor(true),
		WithSATDAdjustSeverity(false),
	)
	debtsWithVendor, err := analyzerWithVendor.AnalyzeFile(vendorFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debtsWithVendor) != 1 {
		t.Errorf("Expected 1 debt item when including vendor, got %d", len(debtsWithVendor))
	}
}

func TestSATD_MinifiedExclusion(t *testing.T) {
	tmpDir := t.TempDir()

	minFile := filepath.Join(tmpDir, "app.min.js")
	content := "// TODO: minified todo\n"
	if err := os.WriteFile(minFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(minFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 0 {
		t.Errorf("Expected 0 debt items for minified file, got %d", len(debts))
	}
}

func TestSATD_SecurityContextEscalation(t *testing.T) {
	tmpDir := t.TempDir()

	// Test security terms in comment text - use NOTE which has low severity
	// and doesn't itself contain security keywords
	normalFile := filepath.Join(tmpDir, "handler.go")
	content := "// NOTE: check SQL here\n"
	if err := os.WriteFile(normalFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(true),
	)
	debts, err := analyzer.AnalyzeFile(normalFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 1 {
		t.Fatalf("Expected 1 debt item, got %d", len(debts))
	}

	// NOTE is normally Low, mention of "sql" escalates it to Medium
	if debts[0].Severity != models.SeverityMedium {
		t.Errorf("Expected severity Medium (escalated due to security terms), got %s", debts[0].Severity)
	}
}

func TestIsTestFile(t *testing.T) {
	analyzer := NewSATDAnalyzer()

	tests := []struct {
		path     string
		expected bool
	}{
		{"main.go", false},
		{"main_test.go", true},
		{"test_main.py", true},
		{"main_test.py", true},
		{"app.test.js", true},
		{"app.spec.ts", true},
		{"__tests__/app.js", true},
		{"test/helper.go", true},
		{"tests/unit/test.py", true},
		{"spec/model_spec.rb", true},
		{"UserTest.java", true},
		{"lib_test.rs", true},
		{"handler_spec.rb", true},
		{"utils.js", false},
		{"validator.py", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			result := analyzer.isTestFile(tt.path)
			if result != tt.expected {
				t.Errorf("isTestFile(%q) = %v, expected %v", tt.path, result, tt.expected)
			}
		})
	}
}

func TestIsVendorFile(t *testing.T) {
	tests := []struct {
		path     string
		expected bool
	}{
		{"main.go", false},
		{"/project/vendor/lib/lib.go", true},
		{"/project/node_modules/pkg/index.js", true},
		{"/project/third_party/dep/dep.go", true},
		{"/project/external/lib/lib.c", true},
		{"/home/user/.cargo/registry/pkg/lib.rs", true},
		{"/usr/lib/python/site-packages/pkg/mod.py", true},
		{"/project/src/app.go", false},
		{"/project/pkg/util/util.go", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			result := isVendorFile(tt.path)
			if result != tt.expected {
				t.Errorf("isVendorFile(%q) = %v, expected %v", tt.path, result, tt.expected)
			}
		})
	}
}

func TestIsMinifiedFile(t *testing.T) {
	tests := []struct {
		path     string
		expected bool
	}{
		{"app.js", false},
		{"app.min.js", true},
		{"style.css", false},
		{"style.min.css", true},
		{"bundle.min.js", true},
		{"jquery.min.js", true},
		{"app.ts", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			result := isMinifiedFile(tt.path)
			if result != tt.expected {
				t.Errorf("isMinifiedFile(%q) = %v, expected %v", tt.path, result, tt.expected)
			}
		})
	}
}

func TestGenerateContextHash(t *testing.T) {
	hash1 := generateContextHash("file.go", 10, "// TODO: fix this")
	hash2 := generateContextHash("file.go", 10, "// TODO: fix this")
	hash3 := generateContextHash("file.go", 11, "// TODO: fix this")
	hash4 := generateContextHash("other.go", 10, "// TODO: fix this")

	// Same inputs should produce same hash
	if hash1 != hash2 {
		t.Error("Same inputs should produce same hash")
	}

	// Different line should produce different hash
	if hash1 == hash3 {
		t.Error("Different line should produce different hash")
	}

	// Different file should produce different hash
	if hash1 == hash4 {
		t.Error("Different file should produce different hash")
	}

	// Hash should be 32 hex chars (16 bytes)
	if len(hash1) != 32 {
		t.Errorf("Expected 32 char hash, got %d", len(hash1))
	}
}

func TestSeverity_Escalate(t *testing.T) {
	tests := []struct {
		input    models.Severity
		expected models.Severity
	}{
		{models.SeverityLow, models.SeverityMedium},
		{models.SeverityMedium, models.SeverityHigh},
		{models.SeverityHigh, models.SeverityCritical},
		{models.SeverityCritical, models.SeverityCritical},
	}

	for _, tt := range tests {
		t.Run(string(tt.input), func(t *testing.T) {
			result := tt.input.Escalate()
			if result != tt.expected {
				t.Errorf("Escalate(%s) = %s, expected %s", tt.input, result, tt.expected)
			}
		})
	}
}

func TestSeverity_Reduce(t *testing.T) {
	tests := []struct {
		input    models.Severity
		expected models.Severity
	}{
		{models.SeverityCritical, models.SeverityHigh},
		{models.SeverityHigh, models.SeverityMedium},
		{models.SeverityMedium, models.SeverityLow},
		{models.SeverityLow, models.SeverityLow},
	}

	for _, tt := range tests {
		t.Run(string(tt.input), func(t *testing.T) {
			result := tt.input.Reduce()
			if result != tt.expected {
				t.Errorf("Reduce(%s) = %s, expected %s", tt.input, result, tt.expected)
			}
		})
	}
}

// TestPatternClassification mirrors PMAT test_pattern_classification.
// Verifies that comment patterns are correctly classified by category and severity.
func TestPatternClassification(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		category models.DebtCategory
		severity models.Severity
	}{
		{
			name:     "TODO pattern",
			content:  "// TODO: implement error handling\n",
			category: models.DebtRequirement,
			severity: models.SeverityLow,
		},
		{
			name:     "SECURITY pattern",
			content:  "// SECURITY: potential SQL injection\n",
			category: models.DebtSecurity,
			severity: models.SeverityCritical,
		},
		{
			name:     "FIXME pattern",
			content:  "// FIXME: broken logic here\n",
			category: models.DebtDefect,
			severity: models.SeverityHigh,
		},
		{
			name:     "HACK pattern",
			content:  "// HACK: ugly workaround\n",
			category: models.DebtDesign,
			severity: models.SeverityMedium,
		},
		{
			name:     "BUG pattern",
			content:  "// BUG: memory leak\n",
			category: models.DebtDefect,
			severity: models.SeverityHigh,
		},
		{
			name:     "KLUDGE pattern",
			content:  "// KLUDGE: temporary fix\n",
			category: models.DebtDesign,
			severity: models.SeverityMedium,
		},
		{
			name:     "performance issue phrase",
			content:  "// performance issue here\n",
			category: models.DebtPerformance,
			severity: models.SeverityMedium,
		},
		{
			name:     "test disabled phrase",
			content:  "// test is disabled\n",
			category: models.DebtTest,
			severity: models.SeverityMedium,
		},
		{
			name:     "technical debt phrase",
			content:  "// technical debt: refactor needed\n",
			category: models.DebtDesign,
			severity: models.SeverityMedium,
		},
		{
			name:     "code smell phrase",
			content:  "// code smell: long method\n",
			category: models.DebtDesign,
			severity: models.SeverityMedium,
		},
		{
			name:     "workaround phrase",
			content:  "// workaround for library issue\n",
			category: models.DebtDesign,
			severity: models.SeverityLow,
		},
		{
			name:     "optimize pattern",
			content:  "// OPTIMIZE: this loop\n",
			category: models.DebtPerformance,
			severity: models.SeverityLow,
		},
		{
			name:     "slow phrase",
			content:  "// SLOW: algorithm\n",
			category: models.DebtPerformance,
			severity: models.SeverityLow,
		},
		{
			name:     "lowercase todo",
			content:  "// todo: add validation\n",
			category: models.DebtRequirement,
			severity: models.SeverityLow,
		},
		{
			name:     "VULN pattern",
			content:  "// VULN: XSS possible\n",
			category: models.DebtSecurity,
			severity: models.SeverityCritical,
		},
		{
			name:     "CVE pattern",
			content:  "// CVE-2021-1234: patch needed\n",
			category: models.DebtSecurity,
			severity: models.SeverityCritical,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer(
				WithSATDIncludeTests(true),
				WithSATDAdjustSeverity(false),
			)
			debts, err := analyzer.AnalyzeFile(testFile)

			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(debts) != 1 {
				t.Fatalf("Expected 1 debt item, got %d", len(debts))
			}

			if debts[0].Category != tt.category {
				t.Errorf("Expected category %s, got %s", tt.category, debts[0].Category)
			}
			if debts[0].Severity != tt.severity {
				t.Errorf("Expected severity %s, got %s", tt.severity, debts[0].Severity)
			}
		})
	}
}

// TestNoSATDInCleanCode mirrors PMAT test - clean code should return no debt items.
func TestNoSATDInCleanCode(t *testing.T) {
	cleanContent := `package main

// Just a regular comment explaining the code
// This describes what the function does
func main() {
	// Another normal explanatory comment
	println("Hello, World!")
}

// Documentation for the helper function
func helper() {
	// Uses standard algorithm
	doSomething()
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "clean.go")
	if err := os.WriteFile(testFile, []byte(cleanContent), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)

	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(debts) != 0 {
		t.Errorf("Expected 0 debt items in clean code, got %d", len(debts))
		for _, d := range debts {
			t.Logf("Unexpected debt: marker=%s, desc=%s, category=%s at line %d", d.Marker, d.Description, d.Category, d.Line)
		}
	}
}

// TestExtractFromContent mirrors PMAT test_extract_from_content.
// Verifies multiple patterns are detected in mixed content.
func TestExtractFromContent(t *testing.T) {
	content := `
// TODO: implement error handling
func main() {
    // FIXME: this is broken
    let x = 42;
    // HACK: python style workaround
    /* BUG: memory leak */
    // SECURITY: XSS vulnerability
}

// Regular comment
func helper() {
    // Another regular comment
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)

	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(debts) != 5 {
		t.Errorf("Expected 5 debt items, got %d", len(debts))
	}

	// Verify line numbers are sorted
	for i := 1; i < len(debts); i++ {
		if debts[i].Line < debts[i-1].Line {
			t.Errorf("Debts should be sorted by line number: line %d before %d", debts[i-1].Line, debts[i].Line)
		}
	}

	// Verify specific debts found by description
	descriptions := make([]string, len(debts))
	for i, d := range debts {
		descriptions[i] = d.Description
	}

	// Verify expected markers are found
	expectedMarkers := []string{"TODO", "FIXME", "HACK", "BUG", "SECURITY"}
	foundMarkers := make(map[string]bool)
	for _, d := range debts {
		foundMarkers[d.Marker] = true
	}

	for _, expected := range expectedMarkers {
		if !foundMarkers[expected] {
			t.Errorf("Expected to find marker '%s'", expected)
		}
	}

	// Verify at least one debt has a meaningful description
	hasDescription := false
	for _, desc := range descriptions {
		if len(desc) > 0 && desc != "FIXME" && desc != "HACK" && desc != "SECURITY" {
			hasDescription = true
			break
		}
	}
	if !hasDescription {
		t.Error("Expected at least one debt with a meaningful description")
	}
}

// extractCommentContent extracts the comment content from a line for testing.
func extractCommentContent(line string) string {
	trimmed := strings.TrimSpace(line)
	if trimmed == "" {
		return ""
	}

	// Check for C++ style comments
	if idx := strings.Index(trimmed, "//"); idx != -1 {
		return strings.TrimSpace(trimmed[idx+2:])
	}

	// Check for Python style comments
	if idx := strings.Index(trimmed, "#"); idx != -1 {
		return strings.TrimSpace(trimmed[idx+1:])
	}

	// Check for multi-line comments
	if strings.HasPrefix(trimmed, "/*") && strings.HasSuffix(trimmed, "*/") {
		content := trimmed[2 : len(trimmed)-2]
		return strings.TrimSpace(content)
	}

	return ""
}

// TestContextHashStability mirrors PMAT test_context_hash_stability.
// Verifies that context hashes are deterministic and different for different inputs.
func TestContextHashStability(t *testing.T) {
	// Same inputs should produce same hash
	hash1 := generateContextHash("test.go", 42, "TODO: fix this")
	hash2 := generateContextHash("test.go", 42, "TODO: fix this")
	if hash1 != hash2 {
		t.Error("Context hashes should be deterministic")
	}

	// Different line numbers should produce different hashes
	hash3 := generateContextHash("test.go", 43, "TODO: fix this")
	if hash1 == hash3 {
		t.Error("Different line numbers should produce different hashes")
	}

	// Different files should produce different hashes
	hash4 := generateContextHash("other.go", 42, "TODO: fix this")
	if hash1 == hash4 {
		t.Error("Different files should produce different hashes")
	}

	// Different content should produce different hashes
	hash5 := generateContextHash("test.go", 42, "FIXME: fix this")
	if hash1 == hash5 {
		t.Error("Different content should produce different hashes")
	}
}

// TestDebtCategoryAsStr mirrors PMAT test_debt_category_as_str.
func TestDebtCategoryAsStr(t *testing.T) {
	tests := []struct {
		category models.DebtCategory
		expected string
	}{
		{models.DebtDesign, "design"},
		{models.DebtDefect, "defect"},
		{models.DebtRequirement, "requirement"},
		{models.DebtTest, "test"},
		{models.DebtPerformance, "performance"},
		{models.DebtSecurity, "security"},
	}

	for _, tt := range tests {
		t.Run(tt.expected, func(t *testing.T) {
			if string(tt.category) != tt.expected {
				t.Errorf("Expected %s, got %s", tt.expected, string(tt.category))
			}
		})
	}
}

// TestSeverityDisplay mirrors PMAT test for severity string values.
func TestSeverityDisplay(t *testing.T) {
	tests := []struct {
		severity models.Severity
		expected string
	}{
		{models.SeverityCritical, "critical"},
		{models.SeverityHigh, "high"},
		{models.SeverityMedium, "medium"},
		{models.SeverityLow, "low"},
	}

	for _, tt := range tests {
		t.Run(tt.expected, func(t *testing.T) {
			if string(tt.severity) != tt.expected {
				t.Errorf("Expected %s, got %s", tt.expected, string(tt.severity))
			}
		})
	}
}

// TestSATDDetectorInitializationStability mirrors PMAT test_detector_initialization_stability.
// Verifies that multiple detector instances are stable.
func TestSATDDetectorInitializationStability(t *testing.T) {
	for i := 0; i < 10; i++ {
		analyzer := NewSATDAnalyzer()
		if analyzer == nil {
			t.Fatalf("Iteration %d: NewSATDAnalyzer returned nil", i)
		}
		if len(analyzer.patterns) == 0 {
			t.Fatalf("Iteration %d: patterns should not be empty", i)
		}
	}
}

// TestExtractCommentContent mirrors PMAT test_extract_comment_content.
func TestExtractCommentContent(t *testing.T) {
	tests := []struct {
		name     string
		line     string
		expected string
	}{
		{
			name:     "C++ style comment",
			line:     "    // TODO: fix this",
			expected: "TODO: fix this",
		},
		{
			name:     "Python style comment",
			line:     "    # FIXME: broken",
			expected: "FIXME: broken",
		},
		{
			name:     "multi-line comment",
			line:     "/* TODO: implement */",
			expected: "TODO: implement",
		},
		{
			name:     "no comment",
			line:     "let x = 42;",
			expected: "",
		},
		{
			name:     "empty line",
			line:     "",
			expected: "",
		},
		{
			name:     "whitespace only",
			line:     "    ",
			expected: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := extractCommentContent(tt.line)
			if result != tt.expected {
				t.Errorf("extractCommentContent(%q) = %q, expected %q", tt.line, result, tt.expected)
			}
		})
	}
}

// TestSeverityAdjustmentContexts mirrors PMAT test_adjust_severity.
// Verifies severity adjustment based on file context.
func TestSeverityAdjustmentContexts(t *testing.T) {
	tmpDir := t.TempDir()

	// Security function context - should escalate
	securityFile := filepath.Join(tmpDir, "validate_input.go")
	content := "// TODO: add validation\n"
	if err := os.WriteFile(securityFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(true),
	)
	debts, err := analyzer.AnalyzeFile(securityFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 1 {
		t.Fatalf("Expected 1 debt, got %d", len(debts))
	}
	// TODO is normally Low, security-related file should escalate to Medium
	if debts[0].Severity != models.SeverityMedium {
		t.Errorf("Expected Medium severity for security context, got %s", debts[0].Severity)
	}

	// Test function context - should reduce severity
	testFile := filepath.Join(tmpDir, "feature_test.go")
	testContent := "// FIXME: broken test\n"
	if err := os.WriteFile(testFile, []byte(testContent), 0644); err != nil {
		t.Fatal(err)
	}

	debts2, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts2) != 1 {
		t.Fatalf("Expected 1 debt, got %d", len(debts2))
	}
	// FIXME is normally High, test file should reduce to Medium
	if debts2[0].Severity != models.SeverityMedium {
		t.Errorf("Expected Medium severity for test context, got %s", debts2[0].Severity)
	}
}

// TestExtractFromContentEmptyString mirrors PMAT test_extract_from_content_empty_string.
func TestExtractFromContentEmptyString(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "empty.go")
	if err := os.WriteFile(testFile, []byte(""), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 0 {
		t.Errorf("Expected 0 debts for empty file, got %d", len(debts))
	}
}

// TestExtractFromContentNoDebt mirrors PMAT test_extract_from_content_no_debt.
func TestExtractFromContentNoDebt(t *testing.T) {
	cleanContent := `package main

func main() {
	println("Hello, world!")
}

type MyStruct struct {
	field int
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "clean.go")
	if err := os.WriteFile(testFile, []byte(cleanContent), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 0 {
		t.Errorf("Expected 0 debts for clean code, got %d", len(debts))
	}
}

// TestExtractFromContentSingleTodo mirrors PMAT test_extract_from_content_single_todo.
func TestExtractFromContentSingleTodo(t *testing.T) {
	content := `package main

func main() {
	// TODO: Implement error handling
	println("Hello, world!")
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "todo.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 1 {
		t.Fatalf("Expected 1 debt, got %d", len(debts))
	}
	if debts[0].Category != models.DebtRequirement {
		t.Errorf("Expected category Requirement, got %s", debts[0].Category)
	}
	if !strings.Contains(debts[0].Description, "Implement error handling") {
		t.Errorf("Expected description to contain 'Implement error handling', got %s", debts[0].Description)
	}
}

// TestExtractFromContentMultipleDebtTypes mirrors PMAT test_extract_from_content_multiple_debt_types.
func TestExtractFromContentMultipleDebtTypes(t *testing.T) {
	content := `package main

func main() {
	// TODO: Add proper error handling
	// FIXME: This algorithm is inefficient
	// HACK: Temporary workaround for issue #123
	// XXX: This code is problematic
	println("Hello, world!")
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "mixed.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 4 {
		t.Fatalf("Expected 4 debts, got %d", len(debts))
	}

	// Check different debt types are detected
	foundMarkers := make(map[string]bool)
	for _, d := range debts {
		foundMarkers[d.Marker] = true
	}

	expectedMarkers := []string{"TODO", "FIXME", "HACK", "XXX"}
	for _, marker := range expectedMarkers {
		if !foundMarkers[marker] {
			t.Errorf("Expected to find marker %s", marker)
		}
	}
}

// TestExtractFromContentCaseInsensitive mirrors PMAT test_extract_from_content_case_insensitive.
func TestExtractFromContentCaseInsensitive(t *testing.T) {
	content := `package main

func test() {
	// todo: lowercase todo
	// Todo: Capitalized todo
	// TODO: All caps todo
	// tOdO: Mixed case todo
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "case.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	// All variations should be detected
	if len(debts) != 4 {
		t.Errorf("Expected 4 debts (all case variations), got %d", len(debts))
	}
}

// TestAnalyzeDirectoryEmpty mirrors PMAT test_analyze_directory_empty.
func TestAnalyzeDirectoryEmpty(t *testing.T) {
	tmpDir := t.TempDir()

	analyzer := NewSATDAnalyzer()
	analysis, err := analyzer.AnalyzeProject([]string{})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}
	if analysis.Summary.TotalItems != 0 {
		t.Errorf("Expected 0 debts for empty directory, got %d", analysis.Summary.TotalItems)
	}

	// Also test with empty directory path
	emptyDir := filepath.Join(tmpDir, "empty")
	if err := os.MkdirAll(emptyDir, 0755); err != nil {
		t.Fatal(err)
	}
	// No files to analyze
	analysis2, err := analyzer.AnalyzeProject([]string{})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}
	if analysis2.Summary.TotalItems != 0 {
		t.Errorf("Expected 0 debts, got %d", analysis2.Summary.TotalItems)
	}
}

// TestAnalyzeDirectoryWithSourceFiles mirrors PMAT test_analyze_directory_with_rust_files.
func TestAnalyzeDirectoryWithSourceFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files without "test" in their names
	file1 := filepath.Join(tmpDir, "file1.go")
	if err := os.WriteFile(file1, []byte("// TODO: Test debt in file 1\nfunc main() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	file2 := filepath.Join(tmpDir, "file2.go")
	if err := os.WriteFile(file2, []byte("// FIXME: Test debt in file 2\nfunc helper() {}"), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	analysis, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}
	if analysis.Summary.TotalItems != 2 {
		t.Errorf("Expected 2 debts, got %d", analysis.Summary.TotalItems)
	}
}

// TestAnalyzeDirectoryIgnoresNonSourceFiles mirrors PMAT test_analyze_directory_ignores_non_source_files.
func TestAnalyzeDirectoryIgnoresNonSourceFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create source file with debt
	sourceFile := filepath.Join(tmpDir, "source.go")
	if err := os.WriteFile(sourceFile, []byte("// TODO: This should be found"), 0644); err != nil {
		t.Fatal(err)
	}

	// Create non-source file with debt (should be ignored by scanner, not by analyzer directly)
	textFile := filepath.Join(tmpDir, "readme.txt")
	if err := os.WriteFile(textFile, []byte("TODO: This should be ignored"), 0644); err != nil {
		t.Fatal(err)
	}

	// Only pass source file to analyzer (simulating scanner behavior)
	analyzer := NewSATDAnalyzer()
	analysis, err := analyzer.AnalyzeProject([]string{sourceFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}
	if analysis.Summary.TotalItems != 1 {
		t.Errorf("Expected 1 debt (only source file), got %d", analysis.Summary.TotalItems)
	}
}

// TestGenerateMetricsEdgeCases mirrors PMAT test_generate_metrics_edge_cases.
func TestGenerateMetricsEdgeCases(t *testing.T) {
	analyzer := NewSATDAnalyzer()

	// Test with empty file list
	analysis, err := analyzer.AnalyzeProject([]string{})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.Summary.TotalItems != 0 {
		t.Errorf("Expected 0 total items, got %d", analysis.Summary.TotalItems)
	}
	if len(analysis.Summary.BySeverity) != 0 {
		t.Errorf("Expected empty BySeverity map, got %d entries", len(analysis.Summary.BySeverity))
	}
	if len(analysis.Summary.ByCategory) != 0 {
		t.Errorf("Expected empty ByCategory map, got %d entries", len(analysis.Summary.ByCategory))
	}
}

// TestGenerateMetricsWithMixedSeverities mirrors PMAT test_generate_metrics_with_mixed_severities.
func TestGenerateMetricsWithMixedSeverities(t *testing.T) {
	tmpDir := t.TempDir()

	content := `// TODO: Low severity
// HACK: Medium severity
// FIXME: High severity
// SECURITY: Critical severity
`
	testFile := filepath.Join(tmpDir, "mixed.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
	)
	analysis, err := analyzer.AnalyzeProject([]string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.Summary.TotalItems != 4 {
		t.Errorf("Expected 4 total items, got %d", analysis.Summary.TotalItems)
	}

	// Check severity distribution
	if analysis.Summary.BySeverity[string(models.SeverityLow)] != 1 {
		t.Errorf("Expected 1 low severity, got %d", analysis.Summary.BySeverity[string(models.SeverityLow)])
	}
	if analysis.Summary.BySeverity[string(models.SeverityMedium)] != 1 {
		t.Errorf("Expected 1 medium severity, got %d", analysis.Summary.BySeverity[string(models.SeverityMedium)])
	}
	if analysis.Summary.BySeverity[string(models.SeverityHigh)] != 1 {
		t.Errorf("Expected 1 high severity, got %d", analysis.Summary.BySeverity[string(models.SeverityHigh)])
	}
	if analysis.Summary.BySeverity[string(models.SeverityCritical)] != 1 {
		t.Errorf("Expected 1 critical severity, got %d", analysis.Summary.BySeverity[string(models.SeverityCritical)])
	}

	// Check category distribution
	if analysis.Summary.ByCategory[string(models.DebtRequirement)] != 1 {
		t.Errorf("Expected 1 requirement category, got %d", analysis.Summary.ByCategory[string(models.DebtRequirement)])
	}
	if analysis.Summary.ByCategory[string(models.DebtDesign)] != 1 {
		t.Errorf("Expected 1 design category, got %d", analysis.Summary.ByCategory[string(models.DebtDesign)])
	}
	if analysis.Summary.ByCategory[string(models.DebtDefect)] != 1 {
		t.Errorf("Expected 1 defect category, got %d", analysis.Summary.ByCategory[string(models.DebtDefect)])
	}
	if analysis.Summary.ByCategory[string(models.DebtSecurity)] != 1 {
		t.Errorf("Expected 1 security category, got %d", analysis.Summary.ByCategory[string(models.DebtSecurity)])
	}
}

// TestMarkdownHeadersNotFlaggedAsSATD mirrors PMAT test_markdown_headers_not_flagged_as_satd.
// Markdown headers like "### Security" should NOT be flagged as SATD.
func TestMarkdownHeadersNotFlaggedAsSATD(t *testing.T) {
	changelogTemplate := `# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

### Changed

### Deprecated

### Removed

### Fixed

### Security
`
	tmpDir := t.TempDir()
	changelogFile := filepath.Join(tmpDir, "CHANGELOG.md")
	if err := os.WriteFile(changelogFile, []byte(changelogTemplate), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(changelogFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Markdown headers should not be flagged as SATD
	securityCount := 0
	for _, d := range debts {
		if d.Category == models.DebtSecurity {
			securityCount++
		}
	}

	if securityCount > 0 {
		t.Errorf("Markdown headers like '### Security' should NOT be flagged as SATD. Found %d false positives", securityCount)
	}
}

// TestBugTrackingIDsNotFlaggedAsSATD mirrors PMAT test_bug_tracking_ids_not_flagged_as_satd.
// Bug tracking IDs like "BUG-012:" should NOT be flagged as SATD.
func TestBugTrackingIDsNotFlaggedAsSATD(t *testing.T) {
	content := `package main

// BUG-012: Apply language override if specified
func applyOverride() {}

// BUG-064 FIX: Uses atomic write operations to prevent file corruption
func atomicWrite() {}

// PMAT-BUG-001: TypeScript class methods must be extracted
func extractMethods() {}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "tracking.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Bug tracking IDs should not be flagged as BUG SATD
	bugCount := 0
	for _, d := range debts {
		if d.Marker == "BUG" {
			bugCount++
			t.Logf("False positive: %s at line %d", d.Description, d.Line)
		}
	}

	if bugCount > 0 {
		t.Errorf("Bug tracking IDs like 'BUG-012:' should NOT be flagged as SATD. Found %d false positives", bugCount)
	}
}

// TestSeverityWeight mirrors PMAT severity weight behavior.
func TestSeverityWeight(t *testing.T) {
	tests := []struct {
		severity models.Severity
		expected int
	}{
		{models.SeverityCritical, 4},
		{models.SeverityHigh, 3},
		{models.SeverityMedium, 2},
		{models.SeverityLow, 1},
	}

	for _, tt := range tests {
		t.Run(string(tt.severity), func(t *testing.T) {
			weight := tt.severity.Weight()
			if weight != tt.expected {
				t.Errorf("Expected weight %d for %s, got %d", tt.expected, tt.severity, weight)
			}
		})
	}
}

// TestTechnicalDebtEquality mirrors PMAT test_technical_debt_equality.
func TestTechnicalDebtEquality(t *testing.T) {
	debt1 := models.TechnicalDebt{
		Category:    models.DebtDesign,
		Severity:    models.SeverityMedium,
		File:        "test.go",
		Line:        10,
		Description: "test debt",
		Marker:      "HACK",
	}
	debt2 := models.TechnicalDebt{
		Category:    models.DebtDesign,
		Severity:    models.SeverityMedium,
		File:        "test.go",
		Line:        10,
		Description: "test debt",
		Marker:      "HACK",
	}

	// Same values should be equal
	if debt1.Category != debt2.Category ||
		debt1.Severity != debt2.Severity ||
		debt1.File != debt2.File ||
		debt1.Line != debt2.Line {
		t.Error("Debt items with same values should be equal")
	}

	// Different category should not be equal
	debt3 := models.TechnicalDebt{
		Category:    models.DebtDefect,
		Severity:    models.SeverityHigh,
		File:        "test.go",
		Line:        10,
		Description: "test debt",
		Marker:      "BUG",
	}
	if debt1.Category == debt3.Category {
		t.Error("Debt items with different categories should not be equal")
	}
}

// TestSATDSummaryCreation mirrors PMAT test_satd_summary_creation.
func TestSATDSummaryCreation(t *testing.T) {
	summary := models.NewSATDSummary()

	// Add items
	summary.AddItem(models.TechnicalDebt{
		Category: models.DebtDesign,
		Severity: models.SeverityHigh,
		File:     "test.go",
	})
	summary.AddItem(models.TechnicalDebt{
		Category: models.DebtDesign,
		Severity: models.SeverityLow,
		File:     "test.go",
	})
	summary.AddItem(models.TechnicalDebt{
		Category: models.DebtDefect,
		Severity: models.SeverityHigh,
		File:     "other.go",
	})

	if summary.TotalItems != 3 {
		t.Errorf("Expected 3 total items, got %d", summary.TotalItems)
	}
	if summary.BySeverity[string(models.SeverityHigh)] != 2 {
		t.Errorf("Expected 2 high severity, got %d", summary.BySeverity[string(models.SeverityHigh)])
	}
	if summary.ByCategory[string(models.DebtDesign)] != 2 {
		t.Errorf("Expected 2 design category, got %d", summary.ByCategory[string(models.DebtDesign)])
	}
	if len(summary.ByFile) != 2 {
		t.Errorf("Expected 2 files with debt, got %d", len(summary.ByFile))
	}
}

// TestAnalyzeProject mirrors PMAT test_analyze_project.
// Tests analyzing a project with multiple files containing different types of SATD
// and verifies the summary statistics are correct.
func TestAnalyzeProject(t *testing.T) {
	tmpDir := t.TempDir()

	file1Content := "// TODO: task 1\n// FIXME: bug 1\n"
	file1 := filepath.Join(tmpDir, "file1.go")
	if err := os.WriteFile(file1, []byte(file1Content), 0644); err != nil {
		t.Fatal(err)
	}

	file2Content := "// HACK: workaround\n// SECURITY: vulnerability\n"
	file2 := filepath.Join(tmpDir, "file2.go")
	if err := os.WriteFile(file2, []byte(file2Content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
	)
	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(result.Items) != 4 {
		t.Errorf("Expected 4 debt items, got %d", len(result.Items))
	}

	// Verify summary statistics
	if result.Summary.TotalItems != 4 {
		t.Errorf("Expected 4 total items in summary, got %d", result.Summary.TotalItems)
	}

	// Verify severity distribution
	expectedSeverities := map[models.Severity]int{
		models.SeverityLow:      1, // TODO
		models.SeverityMedium:   1, // HACK
		models.SeverityHigh:     1, // FIXME
		models.SeverityCritical: 1, // SECURITY
	}
	for sev, count := range expectedSeverities {
		got := result.Summary.BySeverity[string(sev)]
		if got != count {
			t.Errorf("Expected %d %s severity items, got %d", count, sev, got)
		}
	}

	// Verify category distribution
	expectedCategories := map[models.DebtCategory]int{
		models.DebtRequirement: 1, // TODO
		models.DebtDesign:      1, // HACK
		models.DebtDefect:      1, // FIXME
		models.DebtSecurity:    1, // SECURITY
	}
	for cat, count := range expectedCategories {
		got := result.Summary.ByCategory[string(cat)]
		if got != count {
			t.Errorf("Expected %d %s category items, got %d", count, cat, got)
		}
	}

	// Verify file distribution
	if result.Summary.ByFile[file1] != 2 {
		t.Errorf("Expected 2 items in file1, got %d", result.Summary.ByFile[file1])
	}
	if result.Summary.ByFile[file2] != 2 {
		t.Errorf("Expected 2 items in file2, got %d", result.Summary.ByFile[file2])
	}
}

// TestAnalyzeDirectory mirrors PMAT test_analyze_directory.
// Tests that analyzing a directory correctly finds SATD in source files
// but excludes test files when configured.
func TestAnalyzeDirectory(t *testing.T) {
	tmpDir := t.TempDir()

	mainContent := "// TODO: implement feature\n// FIXME: bug here\n"
	mainFile := filepath.Join(tmpDir, "main.go")
	if err := os.WriteFile(mainFile, []byte(mainContent), 0644); err != nil {
		t.Fatal(err)
	}

	helperContent := "// TODO: test helper function needed\n"
	helperFile := filepath.Join(tmpDir, "main_test.go")
	if err := os.WriteFile(helperFile, []byte(helperContent), 0644); err != nil {
		t.Fatal(err)
	}

	// With test files excluded
	analyzerNoTests := NewSATDAnalyzer(
		WithSATDIncludeTests(false),
		WithSATDAdjustSeverity(false),
	)
	resultNoTests, err := analyzerNoTests.AnalyzeProject([]string{mainFile, helperFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Should only find debts from main.go
	if len(resultNoTests.Items) != 2 {
		t.Errorf("Expected 2 debt items when excluding tests, got %d", len(resultNoTests.Items))
	}
	if resultNoTests.Summary.TotalItems != 2 {
		t.Errorf("Expected 2 total items when excluding tests, got %d", resultNoTests.Summary.TotalItems)
	}

	// Verify only main.go debts are included
	for _, debt := range resultNoTests.Items {
		if debt.File != mainFile {
			t.Errorf("Expected all debts from %s, got debt from %s", mainFile, debt.File)
		}
	}

	// With test files included
	analyzerWithTests := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
	)
	resultWithTests, err := analyzerWithTests.AnalyzeProject([]string{mainFile, helperFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Should find debts from both files
	if len(resultWithTests.Items) != 3 {
		t.Errorf("Expected 3 debt items when including tests, got %d", len(resultWithTests.Items))
	}
	if resultWithTests.Summary.TotalItems != 3 {
		t.Errorf("Expected 3 total items when including tests, got %d", resultWithTests.Summary.TotalItems)
	}
}

// TestIsSourceFile mirrors PMAT test_is_source_file.
// Tests that source file detection works correctly for various extensions.
func TestIsSourceFile(t *testing.T) {
	tests := []struct {
		path         string
		isSource     bool
		detectedLang parser.Language
	}{
		// Go
		{path: "main.go", isSource: true, detectedLang: parser.LangGo},
		{path: "pkg/models/debt.go", isSource: true, detectedLang: parser.LangGo},
		// JavaScript/TypeScript
		{path: "app.js", isSource: true, detectedLang: parser.LangJavaScript},
		{path: "app.ts", isSource: true, detectedLang: parser.LangTypeScript},
		{path: "component.tsx", isSource: true, detectedLang: parser.LangTSX},
		{path: "component.jsx", isSource: true, detectedLang: parser.LangTSX}, // JSX uses TSX parser
		// Python
		{path: "main.py", isSource: true, detectedLang: parser.LangPython},
		// Rust
		{path: "lib.rs", isSource: true, detectedLang: parser.LangRust},
		// Java
		{path: "Main.java", isSource: true, detectedLang: parser.LangJava},
		// C/C++
		{path: "main.c", isSource: true, detectedLang: parser.LangC},
		{path: "main.cpp", isSource: true, detectedLang: parser.LangCPP},
		{path: "main.cc", isSource: true, detectedLang: parser.LangCPP},
		{path: "header.h", isSource: true, detectedLang: parser.LangC},
		{path: "header.hpp", isSource: true, detectedLang: parser.LangCPP},
		// C#
		{path: "Program.cs", isSource: true, detectedLang: parser.LangCSharp},
		// Ruby
		{path: "app.rb", isSource: true, detectedLang: parser.LangRuby},
		// PHP
		{path: "index.php", isSource: true, detectedLang: parser.LangPHP},
		// Shell
		{path: "script.sh", isSource: true, detectedLang: parser.LangBash},
		{path: "script.bash", isSource: true, detectedLang: parser.LangBash},
		// Non-source files
		{path: "README.md", isSource: false, detectedLang: parser.LangUnknown},
		{path: "config.json", isSource: false, detectedLang: parser.LangUnknown},
		{path: "config.yaml", isSource: false, detectedLang: parser.LangUnknown},
		{path: "config.toml", isSource: false, detectedLang: parser.LangUnknown},
		{path: "Makefile", isSource: false, detectedLang: parser.LangUnknown},
		{path: "Dockerfile", isSource: true, detectedLang: parser.LangBash}, // Omen parses Dockerfiles as bash
		{path: "image.png", isSource: false, detectedLang: parser.LangUnknown},
		{path: ".gitignore", isSource: false, detectedLang: parser.LangUnknown},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			lang := parser.DetectLanguage(tt.path)
			isSource := lang != parser.LangUnknown

			if isSource != tt.isSource {
				t.Errorf("IsSourceFile(%q) = %v, want %v", tt.path, isSource, tt.isSource)
			}
			if lang != tt.detectedLang {
				t.Errorf("DetectLanguage(%q) = %v, want %v", tt.path, lang, tt.detectedLang)
			}
		})
	}
}

// TestFindSourceFilesExcludesCommonDirs mirrors PMAT test_find_source_files_excludes_common_dirs.
// Tests that common directories like vendor, node_modules, third_party are excluded.
func TestFindSourceFilesExcludesCommonDirs(t *testing.T) {
	tmpDir := t.TempDir()

	// Create directories that should be excluded
	excludedDirs := []string{
		"vendor",
		"node_modules",
		"third_party",
	}

	var excludedFiles []string
	for _, dir := range excludedDirs {
		dirPath := filepath.Join(tmpDir, dir)
		if err := os.MkdirAll(dirPath, 0755); err != nil {
			t.Fatal(err)
		}
		// Create a source file in each excluded directory
		filePath := filepath.Join(dirPath, "excluded.go")
		content := "// TODO: this should be excluded\n"
		if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
		excludedFiles = append(excludedFiles, filePath)
	}

	// Create a source file in the root that should be included
	mainFile := filepath.Join(tmpDir, "main.go")
	mainContent := "// TODO: this should be included\n"
	if err := os.WriteFile(mainFile, []byte(mainContent), 0644); err != nil {
		t.Fatal(err)
	}

	// Test that the analyzer's shouldExcludeFile correctly identifies excluded directories
	analyzer := NewSATDAnalyzer()

	// Verify files in excluded directories are flagged by shouldExcludeFile
	for i, excludedFile := range excludedFiles {
		if !analyzer.shouldExcludeFile(excludedFile) {
			t.Errorf("File in %s directory should be excluded: %s", excludedDirs[i], excludedFile)
		}
	}

	// Verify the main file is NOT excluded
	if analyzer.shouldExcludeFile(mainFile) {
		t.Errorf("Main file should not be excluded: %s", mainFile)
	}

	// Verify analyzing excluded files returns no results (files are skipped)
	for _, excludedFile := range excludedFiles {
		debts, err := analyzer.AnalyzeFile(excludedFile)
		if err != nil {
			t.Fatalf("AnalyzeFile failed: %v", err)
		}
		if len(debts) != 0 {
			t.Errorf("Expected 0 debts for excluded file, got %d: %s", len(debts), excludedFile)
		}
	}

	// Verify analyzing the main file works
	debts, err := analyzer.AnalyzeFile(mainFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}
	if len(debts) != 1 {
		t.Errorf("Expected 1 debt from main file, got %d", len(debts))
	}
}

// TestCategoryMetrics mirrors PMAT test_category_metrics.
// Tests that category-level metrics are computed correctly.
func TestCategoryMetrics(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a file with various debt categories
	content := `// TODO: add feature
// FIXME: critical bug
// HACK: temporary workaround
// BUG: known issue
// SECURITY: potential vulnerability
// OPTIMIZE: performance issue
// FAILING: test disabled
`
	file := filepath.Join(tmpDir, "metrics.go")
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(false),
	)

	// Use AnalyzeProject to get the full analysis with summary
	result, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Verify category distribution
	expectedCategories := map[string]int{
		string(models.DebtRequirement): 1, // TODO
		string(models.DebtDefect):      2, // FIXME, BUG
		string(models.DebtDesign):      1, // HACK
		string(models.DebtSecurity):    1, // SECURITY
		string(models.DebtPerformance): 1, // OPTIMIZE
		string(models.DebtTest):        1, // FAILING
	}

	for cat, expectedCount := range expectedCategories {
		actualCount := result.Summary.ByCategory[cat]
		if actualCount != expectedCount {
			t.Errorf("Category %s: expected %d items, got %d", cat, expectedCount, actualCount)
		}
	}

	// Verify total
	if result.Summary.TotalItems != 7 {
		t.Errorf("Expected 7 total items, got %d", result.Summary.TotalItems)
	}
}

// TestSATDMetrics mirrors PMAT test_satd_metrics.
// Tests the overall SATD metrics structure and computation.
func TestSATDMetrics(t *testing.T) {
	tmpDir := t.TempDir()

	// Create multiple files with different debt patterns
	files := map[string]string{
		"file1.go": "// TODO: task 1\n// TODO: task 2\n",
		"file2.go": "// FIXME: bug 1\n// HACK: workaround\n",
		"file3.go": "// SECURITY: vulnerability\n",
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(true),
		WithSATDAdjustSeverity(true),
	)
	result, err := analyzer.AnalyzeProject(filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Verify total items
	if result.Summary.TotalItems != 5 {
		t.Errorf("Expected 5 total items, got %d", result.Summary.TotalItems)
	}

	// Verify by-file distribution
	if len(result.Summary.ByFile) != 3 {
		t.Errorf("Expected 3 files in ByFile, got %d", len(result.Summary.ByFile))
	}

	// Verify by-severity distribution exists
	if len(result.Summary.BySeverity) == 0 {
		t.Error("BySeverity should not be empty")
	}

	// Verify by-category distribution exists
	if len(result.Summary.ByCategory) == 0 {
		t.Error("ByCategory should not be empty")
	}

	// Verify each item has required fields
	for _, item := range result.Items {
		if item.File == "" {
			t.Error("Item File should not be empty")
		}
		if item.Line == 0 {
			t.Error("Item Line should not be zero")
		}
		if item.Marker == "" {
			t.Error("Item Marker should not be empty")
		}
		if item.Category == "" {
			t.Error("Item Category should not be empty")
		}
		if item.Severity == "" {
			t.Error("Item Severity should not be empty")
		}
		if item.ContextHash == "" {
			t.Error("Item ContextHash should not be empty (GenerateContextID=true)")
		}
	}

	// Verify files analyzed count
	if result.TotalFilesAnalyzed != 3 {
		t.Errorf("Expected 3 files analyzed, got %d", result.TotalFilesAnalyzed)
	}

	// Verify files with debt count
	if result.FilesWithDebt != 3 {
		t.Errorf("Expected 3 files with debt, got %d", result.FilesWithDebt)
	}
}

// TestDebtEvolution mirrors PMAT test_debt_evolution.
// Tests that debt tracking across time periods works correctly.
func TestDebtEvolution(t *testing.T) {
	// This test verifies that the context hash provides stable identity
	// for tracking debt items over time (simulating evolution tracking)

	tmpDir := t.TempDir()
	content := "// TODO: implement feature X\n// FIXME: handle edge case\n"

	file1 := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(file1, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(false),
		WithSATDAdjustSeverity(false),
	)

	// Analyze same file twice
	result1, err := analyzer.AnalyzeFile(file1)
	if err != nil {
		t.Fatalf("First AnalyzeFile failed: %v", err)
	}

	result2, err := analyzer.AnalyzeFile(file1)
	if err != nil {
		t.Fatalf("Second AnalyzeFile failed: %v", err)
	}

	// Verify same items produce same context hashes (identity tracking)
	if len(result1) != len(result2) {
		t.Fatalf("Expected same number of items: %d vs %d", len(result1), len(result2))
	}

	for i := range result1 {
		if result1[i].ContextHash != result2[i].ContextHash {
			t.Errorf("Context hash mismatch for item %d: %s vs %s",
				i, result1[i].ContextHash, result2[i].ContextHash)
		}
	}

	// Verify different files produce different hashes
	file2 := filepath.Join(tmpDir, "different.go")
	if err := os.WriteFile(file2, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	result3, err := analyzer.AnalyzeFile(file2)
	if err != nil {
		t.Fatalf("Third AnalyzeFile failed: %v", err)
	}

	for i := range result1 {
		if result1[i].ContextHash == result3[i].ContextHash {
			t.Errorf("Expected different context hash for different file at item %d", i)
		}
	}
}

// TestASTNodeTypeEquality mirrors PMAT test_ast_node_type_equality.
// Tests that AST node types are correctly identified for SATD extraction.
func TestASTNodeTypeEquality(t *testing.T) {
	// Test that comment node types are correctly handled across languages
	tests := []struct {
		lang    string
		content string
		hasDebt bool
	}{
		// Single-line comments
		{lang: "go", content: "// TODO: task\n", hasDebt: true},
		{lang: "python", content: "# TODO: task\n", hasDebt: true},
		{lang: "rust", content: "// TODO: task\n", hasDebt: true},
		{lang: "java", content: "// TODO: task\n", hasDebt: true},
		{lang: "javascript", content: "// TODO: task\n", hasDebt: true},
		{lang: "ruby", content: "# TODO: task\n", hasDebt: true},
		{lang: "php", content: "// TODO: task\n", hasDebt: true},
		{lang: "bash", content: "# TODO: task\n", hasDebt: true},
		// Block comments
		{lang: "go", content: "/* TODO: task */\n", hasDebt: true},
		{lang: "java", content: "/* TODO: task */\n", hasDebt: true},
		{lang: "javascript", content: "/* TODO: task */\n", hasDebt: true},
		{lang: "c", content: "/* TODO: task */\n", hasDebt: true},
		// No debt
		{lang: "go", content: "// regular comment\n", hasDebt: false},
		{lang: "python", content: "# regular comment\n", hasDebt: false},
	}

	analyzer := NewSATDAnalyzer()
	tmpDir := t.TempDir()

	for _, tt := range tests {
		t.Run(tt.lang+"_"+tt.content[:10], func(t *testing.T) {
			ext := languageToExtension(tt.lang)
			filename := filepath.Join(tmpDir, "test"+ext)

			if err := os.WriteFile(filename, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(filename)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if tt.hasDebt && len(items) == 0 {
				t.Errorf("Expected debt in %s content, got none", tt.lang)
			}
			if !tt.hasDebt && len(items) > 0 {
				t.Errorf("Expected no debt in %s content, got %d items", tt.lang, len(items))
			}
		})
	}
}

// languageToExtension maps language names to file extensions for testing.
func languageToExtension(lang string) string {
	switch lang {
	case "go":
		return ".go"
	case "python":
		return ".py"
	case "rust":
		return ".rs"
	case "java":
		return ".java"
	case "javascript":
		return ".js"
	case "typescript":
		return ".ts"
	case "ruby":
		return ".rb"
	case "php":
		return ".php"
	case "c":
		return ".c"
	case "cpp":
		return ".cpp"
	case "csharp":
		return ".cs"
	case "bash":
		return ".sh"
	default:
		return ".txt"
	}
}

// TestLargeFileHandling tests handling of large files.
func TestLargeFileHandling(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "large.go")

	var content strings.Builder
	content.WriteString("package main\n\n")

	// Create a file with 10000+ lines with debt items scattered throughout
	for i := 0; i < 10000; i++ {
		if i%1000 == 0 {
			content.WriteString(fmt.Sprintf("// TODO: Task at line %d\n", i))
		}
		content.WriteString(fmt.Sprintf("func function%d() { return }\n", i))
	}

	if err := os.WriteFile(testFile, []byte(content.String()), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	items, err := analyzer.AnalyzeFile(testFile)

	if err != nil {
		t.Fatalf("Expected no error analyzing large file, got: %v", err)
	}

	// We should find debt items (10 TODOs scattered through the file)
	expectedCount := 10
	if len(items) != expectedCount {
		t.Errorf("Expected %d debt items in large file, got %d", expectedCount, len(items))
	}

	// Verify all found items are TODOs
	for _, item := range items {
		if item.Marker != "TODO" {
			t.Errorf("Expected TODO marker, got %s", item.Marker)
		}
		if item.Category != models.DebtRequirement {
			t.Errorf("Expected DebtRequirement category, got %s", item.Category)
		}
	}
}

// TestSATDLargeFilePerformance tests that performance doesn't degrade badly on large files.
func TestSATDLargeFilePerformance(t *testing.T) {
	if testing.Short() {
		t.Skip("Skipping performance test in short mode")
	}

	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "verylarge.go")

	var content strings.Builder
	content.WriteString("package main\n\n")

	// Create a 50000 line file
	for i := 0; i < 50000; i++ {
		if i%5000 == 0 {
			content.WriteString(fmt.Sprintf("// FIXME: Issue at line %d\n", i))
		}
		content.WriteString(fmt.Sprintf("func function%d() { return }\n", i))
	}

	if err := os.WriteFile(testFile, []byte(content.String()), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()

	start := time.Now()
	items, err := analyzer.AnalyzeFile(testFile)
	duration := time.Since(start)

	if err != nil {
		t.Fatalf("Expected no error analyzing very large file, got: %v", err)
	}

	// Should complete in reasonable time (5 seconds)
	maxDuration := 5 * time.Second
	if duration > maxDuration {
		t.Errorf("Analysis took too long: %v (max: %v)", duration, maxDuration)
	}

	// Verify we found the expected items (10 FIXMEs)
	expectedCount := 10
	if len(items) != expectedCount {
		t.Errorf("Expected %d debt items, got %d", expectedCount, len(items))
	}

	t.Logf("Analyzed %d lines in %v", 50000, duration)
}

// TestSATDUnicodeHandling tests handling of unicode in comments.
func TestSATDUnicodeHandling(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "unicode.go")

	content := `package main

// TODO: 修复这个问题
func chineseComment() {}

// BUG: исправить ошибку
func russianComment() {}

// NOTE: emoji test 🔧
func emojiComment() {}

// TODO: café résumé naïve
func accentedComment() {}

// Regular TODO: normal comment
func normalComment() {}
`

	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	items, err := analyzer.AnalyzeFile(testFile)

	if err != nil {
		t.Fatalf("Expected no error analyzing unicode file, got: %v", err)
	}

	// Should find all 5 debt markers
	expectedCount := 5
	if len(items) != expectedCount {
		t.Errorf("Expected %d debt items with unicode, got %d", expectedCount, len(items))
	}

	// Verify markers are correctly identified
	expectedMarkers := map[string]int{
		"TODO": 3, // Chinese, accented, and normal
		"BUG":  1, // Russian (using BUG which has 1 capture group)
		"NOTE": 1, // Emoji (using NOTE which has 1 capture group)
	}

	foundMarkers := make(map[string]int)
	for _, item := range items {
		foundMarkers[item.Marker]++
	}

	for marker, expectedCount := range expectedMarkers {
		if foundMarkers[marker] != expectedCount {
			t.Errorf("Expected %d %s markers, got %d", expectedCount, marker, foundMarkers[marker])
		}
	}

	// Verify that unicode content is properly preserved in descriptions
	unicodeTests := []struct {
		substring string
		name      string
	}{
		{"修复这个问题", "Chinese characters"},
		{"исправить ошибку", "Russian characters"},
		{"🔧", "emoji"},
		{"café", "accented characters"},
	}

	descriptions := make([]string, len(items))
	for i, item := range items {
		descriptions[i] = item.Description
	}

	for _, tt := range unicodeTests {
		found := false
		for _, desc := range descriptions {
			if strings.Contains(desc, tt.substring) {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Expected to find %s in descriptions, but didn't. Descriptions: %v", tt.name, descriptions)
		}
	}
}

// PMAT-compatible tests

// Category 1: Debt Category and Severity Tests

func TestDebtCategoryDisplay(t *testing.T) {
	tests := []struct {
		name     string
		category models.DebtCategory
		expected string
	}{
		{
			name:     "design category",
			category: models.DebtDesign,
			expected: "design",
		},
		{
			name:     "defect category",
			category: models.DebtDefect,
			expected: "defect",
		},
		{
			name:     "requirement category",
			category: models.DebtRequirement,
			expected: "requirement",
		},
		{
			name:     "test category",
			category: models.DebtTest,
			expected: "test",
		},
		{
			name:     "performance category",
			category: models.DebtPerformance,
			expected: "performance",
		},
		{
			name:     "security category",
			category: models.DebtSecurity,
			expected: "security",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := string(tt.category)
			if result != tt.expected {
				t.Errorf("Expected %q, got %q", tt.expected, result)
			}
		})
	}
}

func TestDebtCategoryVariants(t *testing.T) {
	categories := []models.DebtCategory{
		models.DebtDesign,
		models.DebtDefect,
		models.DebtRequirement,
		models.DebtTest,
		models.DebtPerformance,
		models.DebtSecurity,
	}

	seen := make(map[models.DebtCategory]bool)
	for _, cat := range categories {
		if seen[cat] {
			t.Errorf("Duplicate category found: %s", cat)
		}
		seen[cat] = true
	}

	if len(seen) != 6 {
		t.Errorf("Expected 6 distinct categories, got %d", len(seen))
	}
}

func TestSeverityVariants(t *testing.T) {
	severities := []models.Severity{
		models.SeverityLow,
		models.SeverityMedium,
		models.SeverityHigh,
		models.SeverityCritical,
	}

	seen := make(map[models.Severity]bool)
	for _, sev := range severities {
		if seen[sev] {
			t.Errorf("Duplicate severity found: %s", sev)
		}
		seen[sev] = true
	}

	if len(seen) != 4 {
		t.Errorf("Expected 4 distinct severities, got %d", len(seen))
	}

	expectedWeights := map[models.Severity]int{
		models.SeverityLow:      1,
		models.SeverityMedium:   2,
		models.SeverityHigh:     3,
		models.SeverityCritical: 4,
	}

	for sev, expectedWeight := range expectedWeights {
		if weight := sev.Weight(); weight != expectedWeight {
			t.Errorf("Severity %s: expected weight %d, got %d", sev, expectedWeight, weight)
		}
	}
}

// Category 2: Classifier Tests

func TestDebtClassifierNew(t *testing.T) {
	analyzer := NewSATDAnalyzer()

	if analyzer == nil {
		t.Fatal("NewSATDAnalyzer returned nil")
	}

	if len(analyzer.patterns) == 0 {
		t.Error("Analyzer should have non-empty patterns")
	}

	expectedPatterns := 21
	if len(analyzer.patterns) != expectedPatterns {
		t.Errorf("Expected %d patterns, got %d", expectedPatterns, len(analyzer.patterns))
	}
}

func TestDebtClassifierDefault(t *testing.T) {
	analyzer := NewSATDAnalyzer()

	categoryCounts := make(map[models.DebtCategory]int)
	for _, p := range analyzer.patterns {
		categoryCounts[p.category]++
	}

	if len(categoryCounts) == 0 {
		t.Error("Default classifier should have patterns for multiple categories")
	}

	expectedCategories := []models.DebtCategory{
		models.DebtSecurity,
		models.DebtDefect,
		models.DebtDesign,
		models.DebtRequirement,
		models.DebtTest,
		models.DebtPerformance,
	}

	for _, cat := range expectedCategories {
		if categoryCounts[cat] == 0 {
			t.Errorf("Expected patterns for category %s, found none", cat)
		}
	}
}

// Category 3: Creation Tests

func TestTechnicalDebtCreation(t *testing.T) {
	tests := []struct {
		name        string
		category    models.DebtCategory
		severity    models.Severity
		file        string
		line        uint32
		description string
		marker      string
		text        string
		column      uint32
	}{
		{
			name:        "complete debt item",
			category:    models.DebtDesign,
			severity:    models.SeverityMedium,
			file:        "test.go",
			line:        42,
			description: "refactor this code",
			marker:      "HACK",
			text:        "// HACK: refactor this code",
			column:      5,
		},
		{
			name:        "minimal debt item",
			category:    models.DebtDefect,
			severity:    models.SeverityHigh,
			file:        "bug.go",
			line:        1,
			description: "fix null pointer",
			marker:      "FIXME",
			text:        "",
			column:      0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			debt := models.TechnicalDebt{
				Category:    tt.category,
				Severity:    tt.severity,
				File:        tt.file,
				Line:        tt.line,
				Description: tt.description,
				Marker:      tt.marker,
				Text:        tt.text,
				Column:      tt.column,
			}

			if debt.Category != tt.category {
				t.Errorf("Expected category %s, got %s", tt.category, debt.Category)
			}
			if debt.Severity != tt.severity {
				t.Errorf("Expected severity %s, got %s", tt.severity, debt.Severity)
			}
			if debt.File != tt.file {
				t.Errorf("Expected file %s, got %s", tt.file, debt.File)
			}
			if debt.Line != tt.line {
				t.Errorf("Expected line %d, got %d", tt.line, debt.Line)
			}
			if debt.Description != tt.description {
				t.Errorf("Expected description %s, got %s", tt.description, debt.Description)
			}
			if debt.Marker != tt.marker {
				t.Errorf("Expected marker %s, got %s", tt.marker, debt.Marker)
			}
		})
	}
}

func TestSATDAnalysisResultCreation(t *testing.T) {
	tests := []struct {
		name               string
		items              []models.TechnicalDebt
		totalFilesAnalyzed int
		filesWithDebt      int
	}{
		{
			name: "analysis with items",
			items: []models.TechnicalDebt{
				{
					Category:    models.DebtDesign,
					Severity:    models.SeverityMedium,
					File:        "test.go",
					Line:        1,
					Description: "test debt",
					Marker:      "HACK",
				},
			},
			totalFilesAnalyzed: 5,
			filesWithDebt:      1,
		},
		{
			name:               "empty analysis",
			items:              []models.TechnicalDebt{},
			totalFilesAnalyzed: 10,
			filesWithDebt:      0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			summary := models.NewSATDSummary()
			for _, item := range tt.items {
				summary.AddItem(item)
			}

			analysis := models.SATDAnalysis{
				Items:              tt.items,
				Summary:            summary,
				TotalFilesAnalyzed: tt.totalFilesAnalyzed,
				FilesWithDebt:      tt.filesWithDebt,
			}

			if len(analysis.Items) != len(tt.items) {
				t.Errorf("Expected %d items, got %d", len(tt.items), len(analysis.Items))
			}
			if analysis.TotalFilesAnalyzed != tt.totalFilesAnalyzed {
				t.Errorf("Expected %d files analyzed, got %d", tt.totalFilesAnalyzed, analysis.TotalFilesAnalyzed)
			}
			if analysis.FilesWithDebt != tt.filesWithDebt {
				t.Errorf("Expected %d files with debt, got %d", tt.filesWithDebt, analysis.FilesWithDebt)
			}
			if analysis.Summary.TotalItems != len(tt.items) {
				t.Errorf("Expected summary total %d, got %d", len(tt.items), analysis.Summary.TotalItems)
			}
		})
	}
}

// Category 4: Detector Tests

func TestSATDDetectorCreation(t *testing.T) {
	tests := []struct {
		name            string
		createAnalyzer  func() *SATDAnalyzer
		expectedPattern int
	}{
		{
			name: "default detector",
			createAnalyzer: func() *SATDAnalyzer {
				return NewSATDAnalyzer()
			},
			expectedPattern: 21,
		},
		{
			name: "detector with options",
			createAnalyzer: func() *SATDAnalyzer {
				return NewSATDAnalyzer(WithSATDIncludeTests(true))
			},
			expectedPattern: 21,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := tt.createAnalyzer()

			if analyzer == nil {
				t.Fatal("Analyzer creation returned nil")
			}

			if len(analyzer.patterns) == 0 {
				t.Error("Detector should have non-empty patterns")
			}

			if len(analyzer.patterns) != tt.expectedPattern {
				t.Errorf("Expected %d patterns, got %d", tt.expectedPattern, len(analyzer.patterns))
			}

			for i, p := range analyzer.patterns {
				if p.regex == nil {
					t.Errorf("Pattern %d has nil regex", i)
				}
				if p.category == "" {
					t.Errorf("Pattern %d has empty category", i)
				}
				if p.severity == "" {
					t.Errorf("Pattern %d has empty severity", i)
				}
			}
		})
	}
}

// Category 5: Content Extraction Tests

func TestExtractFromContentComplexTestBlocks(t *testing.T) {
	analyzer := NewSATDAnalyzer()
	tmpDir := t.TempDir()

	tests := []struct {
		name         string
		content      string
		expectedDebt int
		description  string
	}{
		{
			name: "test function with TODO",
			content: `package main

func TestComplexScenario(t *testing.T) {
	// TODO: Add more edge cases
	// Prepare input data
	data := prepareTestData()

	// FIXME: This test is flaky
	result := processData(data)

	if result == nil {
		t.Fatal("expected non-nil result")
	}
}`,
			expectedDebt: 2,
			description:  "Should detect TODO and FIXME in test function",
		},
		{
			name: "nested test blocks with multiple markers",
			content: `package main

func TestSuite(t *testing.T) {
	t.Run("scenario1", func(t *testing.T) {
		// HACK: Workaround for timing issue
		time.Sleep(100 * time.Millisecond)
	})

	t.Run("scenario2", func(t *testing.T) {
		// OPTIMIZE: This could be faster
		for i := 0; i < 1000; i++ {
			process(i)
		}
	})

	// NOTE: These tests need refactoring
}`,
			expectedDebt: 3,
			description:  "Should detect markers in nested test blocks",
		},
		{
			name: "test with table-driven tests",
			content: `package main

func TestTableDriven(t *testing.T) {
	tests := []struct {
		name string
		input int
		want int
	}{
		// TODO: Add negative test cases
		{"positive", 5, 25},
		{"zero", 0, 0},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// FIXME: Handle edge cases
			got := square(tt.input)
			if got != tt.want {
				t.Errorf("got %d, want %d", got, tt.want)
			}
		})
	}
}`,
			expectedDebt: 2,
			description:  "Should detect markers in table-driven tests",
		},
		{
			name: "test with multiple comment types",
			content: `package main

// TestMultiLineComment tests the behavior
// TODO: Expand test coverage
// of the multiline function
func TestMultiLineComment(t *testing.T) {
	/*
	 * HACK: This is a workaround
	 * for a known limitation
	 */
	result := doSomething()

	// SECURITY: Validate input sanitization
	if !validate(result) {
		t.Fatal("validation failed")
	}
}`,
			expectedDebt: 3,
			description:  "Should detect markers in both line and block comments",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) != tt.expectedDebt {
				t.Errorf("%s: expected %d debt items, got %d", tt.description, tt.expectedDebt, len(items))
				for i, item := range items {
					t.Logf("  Item %d: %s at line %d: %s", i+1, item.Marker, item.Line, item.Description)
				}
			}
		})
	}
}

func TestExtractFromContentSkipsTestBlocks(t *testing.T) {
	tmpDir := t.TempDir()

	tests := []struct {
		name          string
		content       string
		includeTests  bool
		expectedCount int
		description   string
	}{
		{
			name: "test file excluded when includeTests is false",
			content: `package main

func TestExample(t *testing.T) {
	// TODO: Add more test cases
	result := doWork()
	// FIXME: This assertion is weak
	assert.NotNil(t, result)
}`,
			includeTests:  false,
			expectedCount: 0,
			description:   "Should skip test file when includeTests=false",
		},
		{
			name: "test file included when includeTests is true",
			content: `package main

func TestExample(t *testing.T) {
	// TODO: Add more test cases
	result := doWork()
	// FIXME: This assertion is weak
	assert.NotNil(t, result)
}`,
			includeTests:  true,
			expectedCount: 2,
			description:   "Should process test file when includeTests=true",
		},
		{
			name: "production code always processed",
			content: `package main

func DoWork() error {
	// TODO: Implement retry logic
	// HACK: Temporary workaround
	return nil
}`,
			includeTests:  false,
			expectedCount: 2,
			description:   "Should process production code regardless of includeTests",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewSATDAnalyzer(
				WithSATDIncludeTests(tt.includeTests),
			)

			var filename string
			if strings.Contains(tt.content, "TestExample") || strings.Contains(tt.content, "TestSuite") {
				filename = "example_test.go"
			} else {
				filename = "example.go"
			}

			testFile := filepath.Join(tmpDir, filename)
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) != tt.expectedCount {
				t.Errorf("%s: expected %d items, got %d", tt.description, tt.expectedCount, len(items))
			}
		})
	}
}

func TestExtractFromContentNonGoFiles(t *testing.T) {
	analyzer := NewSATDAnalyzer()
	tmpDir := t.TempDir()

	tests := []struct {
		name         string
		filename     string
		content      string
		expectedDebt int
		description  string
	}{
		{
			name:     "javascript file with JSDoc",
			filename: "code.js",
			content: `/**
 * TODO: Refactor this function
 * @param {string} input
 */
function process(input) {
	// FIXME: Handle null case
	return input.toUpperCase();
}`,
			expectedDebt: 2,
			description:  "Should detect markers in JavaScript comments",
		},
		{
			name:     "python file with comments",
			filename: "code.py",
			content: `def process_data(data):
    # TODO: Add validation
    # HACK: This is a temporary solution
    # OPTIMIZE: Use numpy for better performance
    return data * 2`,
			expectedDebt: 3,
			description:  "Should detect markers in Python comments",
		},
		{
			name:     "typescript with multiline",
			filename: "code.ts",
			content: `interface User {
	id: number;
	// TODO: Add email validation
	email: string;
}

/*
 * SECURITY: Review authentication logic
 * FIXME: Session timeout too long
 */
class AuthService {
	login(user: User): void {
		// HACK: Bypass for testing
	}
}`,
			expectedDebt: 4,
			description:  "Should detect markers in TypeScript",
		},
		{
			name:     "rust file",
			filename: "code.rs",
			content: `// TODO: Implement error handling
fn calculate(x: i32) -> i32 {
    // FIXME: Overflow not handled
    x * 2
}

/// OPTIMIZE: Use simd instructions
pub fn fast_sum(nums: &[i32]) -> i32 {
    nums.iter().sum()
}`,
			expectedDebt: 3,
			description:  "Should detect markers in Rust",
		},
		{
			name:     "java file",
			filename: "Code.java",
			content: `public class Example {
    /**
     * TODO: Add javadoc
     * FIXME: Thread safety issue
     */
    public void process() {
        // HACK: Temporary workaround
        System.out.println("processing");
    }
}`,
			expectedDebt: 3,
			description:  "Should detect markers in Java",
		},
		{
			name:     "c file",
			filename: "code.c",
			content: `#include <stdio.h>

/* TODO: Add error checking */
int main() {
    // FIXME: Buffer overflow risk
    char buf[10];
    // SECURITY: Validate input
    gets(buf);
    return 0;
}`,
			expectedDebt: 3,
			description:  "Should detect markers in C",
		},
		{
			name:     "ruby file",
			filename: "code.rb",
			content: `class Calculator
  # TODO: Add input validation
  def add(a, b)
    # FIXME: Handle nil values
    a + b
  end

  # OPTIMIZE: Cache results
  def expensive_operation
    sleep(1)
  end
end`,
			expectedDebt: 3,
			description:  "Should detect markers in Ruby",
		},
		{
			name:     "php file",
			filename: "code.php",
			content: `<?php
// TODO: Sanitize input
function processData($input) {
    // SECURITY: SQL injection risk
    $query = "SELECT * FROM users WHERE id = " . $input;
    /* FIXME: Use prepared statements */
    return mysql_query($query);
}
?>`,
			expectedDebt: 3,
			description:  "Should detect markers in PHP",
		},
		{
			name:     "bash script",
			filename: "script.sh",
			content: `#!/bin/bash

# TODO: Add argument validation
# FIXME: Handle missing files
input_file=$1

# SECURITY: Sanitize file path
cat "$input_file" | grep "pattern"`,
			expectedDebt: 3,
			description:  "Should detect markers in Bash",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			testFile := filepath.Join(tmpDir, tt.filename)
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) != tt.expectedDebt {
				t.Errorf("%s: expected %d debt items, got %d", tt.description, tt.expectedDebt, len(items))
				for i, item := range items {
					t.Logf("  Item %d: %s at line %d", i+1, item.Marker, item.Line)
				}
			}
		})
	}
}

func TestExtractFromLineErrorHandling(t *testing.T) {
	analyzer := NewSATDAnalyzer()
	tmpDir := t.TempDir()

	tests := []struct {
		name        string
		content     string
		expectError bool
		expectDebt  bool
		description string
	}{
		{
			name:        "empty file",
			content:     "",
			expectError: false,
			expectDebt:  false,
			description: "Should handle empty file without error",
		},
		{
			name:        "only whitespace",
			content:     "    \n\t\n   \n",
			expectError: false,
			expectDebt:  false,
			description: "Should handle whitespace-only file",
		},
		{
			name:        "empty comment line",
			content:     "//\n",
			expectError: false,
			expectDebt:  false,
			description: "Should handle empty comment",
		},
		{
			name:        "very long line with marker",
			content:     "// TODO: " + strings.Repeat("x", 10000) + "\n",
			expectError: false,
			expectDebt:  true,
			description: "Should handle very long lines",
		},
		{
			name: "file with no comments",
			content: `package main

func main() {
	x := 42
	y := x * 2
	print(y)
}`,
			expectError: false,
			expectDebt:  false,
			description: "Should handle file with no comments",
		},
		{
			name: "malformed comment blocks",
			content: `package main

/* TODO: Unclosed block comment
func main() {
	// FIXME: Inside unclosed block
}
`,
			expectError: false,
			expectDebt:  true,
			description: "Should handle malformed block comments",
		},
		{
			name: "marker at end of line without description",
			content: `package main

// TODO
// FIXME
// HACK
`,
			expectError: false,
			expectDebt:  true,
			description: "Should handle markers without descriptions",
		},
		{
			name: "unicode in comments",
			content: `package main

// TODO: 添加输入验证 (Add input validation)
// FIXME: Исправить ошибку (Fix bug)
// HACK: 🔧 temporary workaround
`,
			expectError: false,
			expectDebt:  true,
			description: "Should handle unicode characters in comments",
		},
		{
			name:        "mixed line endings",
			content:     "// TODO: Unix line\n// FIXME: Windows line\r\n// HACK: Old Mac line\r",
			expectError: false,
			expectDebt:  true,
			description: "Should handle different line endings",
		},
		{
			name:        "null bytes in file",
			content:     "// TODO: Before null\x00After null\n",
			expectError: false,
			expectDebt:  true,
			description: "Should handle null bytes",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(testFile)

			if tt.expectError && err == nil {
				t.Errorf("%s: expected error, got nil", tt.description)
			}

			if !tt.expectError && err != nil {
				t.Errorf("%s: unexpected error: %v", tt.description, err)
			}

			hasDebt := len(items) > 0
			if tt.expectDebt && !hasDebt {
				t.Errorf("%s: expected debt items, got none", tt.description)
			}

			if !tt.expectDebt && hasDebt {
				t.Errorf("%s: expected no debt items, got %d", tt.description, len(items))
			}
		})
	}
}

// Category 6: Column Detection

func TestFindCommentColumn(t *testing.T) {
	tests := []struct {
		name           string
		line           string
		expectedColumn int
		description    string
	}{
		{
			name:           "indented line comment",
			line:           "    // comment",
			expectedColumn: 5,
			description:    "Should find column 5 for indented comment",
		},
		{
			name:           "python hash comment at start",
			line:           "# python comment",
			expectedColumn: 1,
			description:    "Should find column 1 for comment at line start",
		},
		{
			name:           "inline block comment",
			line:           "code; /* comment */",
			expectedColumn: 7,
			description:    "Should find column 7 for inline block comment",
		},
		{
			name:           "html comment",
			line:           "<!-- html comment -->",
			expectedColumn: 1,
			description:    "Should find column 1 for HTML comment",
		},
		{
			name:           "no comment",
			line:           "no comment here",
			expectedColumn: 1,
			description:    "Should return column 1 when no comment found",
		},
		{
			name:           "deep indentation",
			line:           "\t\t\t// deeply indented",
			expectedColumn: 4,
			description:    "Should handle tab indentation (tabs count as 1)",
		},
		{
			name:           "mixed tabs and spaces",
			line:           "\t  // mixed indent",
			expectedColumn: 4,
			description:    "Should handle mixed tab/space indentation",
		},
		{
			name:           "block comment start",
			line:           "  /* start of block",
			expectedColumn: 3,
			description:    "Should find column for block comment start",
		},
		{
			name:           "block comment middle",
			line:           "   * middle of block",
			expectedColumn: 4,
			description:    "Should find column for block comment continuation",
		},
		{
			name:           "block comment end",
			line:           "   */",
			expectedColumn: 4,
			description:    "Should find column for block comment end",
		},
		{
			name:           "multiple comment markers",
			line:           "code // first /* second",
			expectedColumn: 6,
			description:    "Should find first comment marker",
		},
		{
			name:           "comment after assignment",
			line:           `x := 42 // variable assignment`,
			expectedColumn: 9,
			description:    "Should find comment after code",
		},
		{
			name:           "empty line",
			line:           "",
			expectedColumn: 1,
			description:    "Should return 1 for empty line",
		},
		{
			name:           "whitespace only",
			line:           "    ",
			expectedColumn: 1,
			description:    "Should return 1 for whitespace-only line",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			col := findCommentColumn(tt.line)
			if col != tt.expectedColumn {
				t.Errorf("%s: expected column %d, got %d", tt.description, tt.expectedColumn, col)
			}
		})
	}
}

// findCommentColumn returns the 1-indexed column where a comment starts.
// Returns 1 if no comment is found or for empty lines.
func findCommentColumn(line string) int {
	if len(line) == 0 {
		return 1
	}

	// Check for block comment continuation or end first: lines with * or */
	trimmed := strings.TrimSpace(line)
	if strings.HasPrefix(trimmed, "*") {
		// Find position of first * in original line
		for i, ch := range line {
			if ch == '*' {
				return i + 1
			}
		}
	}

	// Try to find line comment markers: //, #
	lineMarkers := []string{"//", "#"}
	minPos := len(line)
	found := false

	for _, marker := range lineMarkers {
		if pos := strings.Index(line, marker); pos != -1 {
			if pos < minPos {
				minPos = pos
				found = true
			}
		}
	}

	// Try to find block comment markers: /*, <!--
	blockMarkers := []string{"/*", "<!--"}
	for _, marker := range blockMarkers {
		if pos := strings.Index(line, marker); pos != -1 {
			if pos < minPos {
				minPos = pos
				found = true
			}
		}
	}

	if !found {
		return 1
	}

	return minPos + 1
}

// ============================================================
// Category 7: File Collection Tests
// ============================================================

// TestCollectFilesRecursiveEmptyDirectory verifies that scanning an empty directory returns no files.
func TestCollectFilesRecursiveEmptyDirectory(t *testing.T) {
	tmpDir := t.TempDir()

	cfg := config.DefaultConfig()
	s := scanner.NewScanner(cfg)

	files, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir failed: %v", err)
	}

	if len(files) != 0 {
		t.Errorf("Expected 0 files in empty directory, got %d", len(files))
	}
}

// TestCollectFilesRecursiveNestedDirectories verifies that scanning finds files in nested subdirectories.
func TestCollectFilesRecursiveNestedDirectories(t *testing.T) {
	tmpDir := t.TempDir()

	// Create nested directory structure
	// root/
	//   src/
	//     main.go
	//     utils/
	//       helper.go
	//   pkg/
	//     models/
	//       user.go

	srcDir := filepath.Join(tmpDir, "src")
	utilsDir := filepath.Join(srcDir, "utils")
	pkgDir := filepath.Join(tmpDir, "pkg")
	modelsDir := filepath.Join(pkgDir, "models")

	if err := os.MkdirAll(utilsDir, 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(modelsDir, 0755); err != nil {
		t.Fatal(err)
	}

	files := map[string]string{
		filepath.Join(srcDir, "main.go"):     "// TODO: implement\npackage main\n",
		filepath.Join(utilsDir, "helper.go"): "// FIXME: bug\npackage utils\n",
		filepath.Join(modelsDir, "user.go"):  "// HACK: temporary\npackage models\n",
	}

	for path, content := range files {
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
	}

	cfg := config.DefaultConfig()
	s := scanner.NewScanner(cfg)

	foundFiles, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir failed: %v", err)
	}

	if len(foundFiles) != 3 {
		t.Errorf("Expected 3 files in nested directories, got %d", len(foundFiles))
	}

	// Verify each file was found
	foundPaths := make(map[string]bool)
	for _, f := range foundFiles {
		foundPaths[f] = true
	}

	for expectedPath := range files {
		if !foundPaths[expectedPath] {
			t.Errorf("Expected file %s not found in scan results", expectedPath)
		}
	}
}

// TestCollectFilesRecursiveSkipsExcludedDirectories verifies that common excluded directories are skipped.
func TestCollectFilesRecursiveSkipsExcludedDirectories(t *testing.T) {
	tmpDir := t.TempDir()

	// Create directories that should be excluded based on DefaultConfig
	excludedDirs := []string{
		"node_modules",
		".git",
		"vendor",
		"__pycache__",
		"dist",
		"build",
	}

	// Also create a non-excluded directory
	srcDir := filepath.Join(tmpDir, "src")
	if err := os.MkdirAll(srcDir, 0755); err != nil {
		t.Fatal(err)
	}

	// Add a file in the source directory
	srcFile := filepath.Join(srcDir, "main.go")
	if err := os.WriteFile(srcFile, []byte("// TODO: test\npackage main\n"), 0644); err != nil {
		t.Fatal(err)
	}

	// Add files in each excluded directory
	for _, dir := range excludedDirs {
		dirPath := filepath.Join(tmpDir, dir)
		if err := os.MkdirAll(dirPath, 0755); err != nil {
			t.Fatal(err)
		}

		// Add a source file in the excluded directory
		filePath := filepath.Join(dirPath, "excluded.go")
		if err := os.WriteFile(filePath, []byte("// TODO: excluded\npackage excluded\n"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	cfg := config.DefaultConfig()
	s := scanner.NewScanner(cfg)

	files, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir failed: %v", err)
	}

	// Should only find the file in src/, not in any excluded directories
	if len(files) != 1 {
		t.Errorf("Expected 1 file (excluded directories should be skipped), got %d", len(files))
		for _, f := range files {
			t.Logf("Found file: %s", f)
		}
	}

	if len(files) > 0 && files[0] != srcFile {
		t.Errorf("Expected to find %s, got %s", srcFile, files[0])
	}

	// Verify none of the excluded directories appear in results
	for _, f := range files {
		for _, excludedDir := range excludedDirs {
			if strings.Contains(f, string(filepath.Separator)+excludedDir+string(filepath.Separator)) {
				t.Errorf("File %s should not be in excluded directory %s", f, excludedDir)
			}
		}
	}
}

// TestCollectFilesRecursiveSkipsTestFiles verifies test file exclusion when IncludeTests=false.
func TestCollectFilesRecursiveSkipsTestFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create both test files and regular files
	files := map[string]bool{
		"main.go":            false, // not a test file
		"main_test.go":       true,  // test file
		"utils.py":           false,
		"test_utils.py":      true,
		"utils_test.py":      true,
		"api.js":             false,
		"api.test.js":        true,
		"api.spec.js":        true,
		"component.tsx":      false,
		"component.test.tsx": true,
	}

	for filename := range files {
		path := filepath.Join(tmpDir, filename)
		if err := os.WriteFile(path, []byte("// TODO: test\n"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	// Test with IncludeTests=false (should skip test files)
	analyzer := NewSATDAnalyzer(
		WithSATDIncludeTests(false),
		WithSATDIncludeVendor(false),
		WithSATDAdjustSeverity(false),
	)

	cfg := config.DefaultConfig()
	s := scanner.NewScanner(cfg)

	allFiles, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir failed: %v", err)
	}

	// Count how many non-test files we should find
	expectedCount := 0
	for _, isTest := range files {
		if !isTest {
			expectedCount++
		}
	}

	// Filter files through analyzer (simulates what AnalyzeProject does)
	var processedFiles []string
	for _, f := range allFiles {
		if !analyzer.shouldExcludeFile(f) {
			processedFiles = append(processedFiles, f)
		}
	}

	if len(processedFiles) != expectedCount {
		t.Errorf("Expected %d non-test files, got %d", expectedCount, len(processedFiles))
		t.Logf("Processed files:")
		for _, f := range processedFiles {
			t.Logf("  %s", f)
		}
	}

	// Verify no test files in results
	for _, f := range processedFiles {
		basename := filepath.Base(f)
		if files[basename] {
			t.Errorf("Test file %s should have been excluded", basename)
		}
	}
}

// TestCollectFilesRecursiveWithSourceFiles verifies finding various source file types.
func TestCollectFilesRecursiveWithSourceFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create files with different supported extensions
	sourceFiles := []struct {
		name    string
		content string
		lang    string
	}{
		{"main.go", "// TODO: Go file\npackage main\n", "Go"},
		{"app.rs", "// TODO: Rust file\nfn main() {}\n", "Rust"},
		{"script.py", "# TODO: Python file\ndef main():\n    pass\n", "Python"},
		{"index.js", "// TODO: JavaScript file\nconsole.log('test');\n", "JavaScript"},
		{"app.ts", "// TODO: TypeScript file\nconst x: string = 'test';\n", "TypeScript"},
		{"component.tsx", "// TODO: TSX file\nconst C = () => <div/>;\n", "TSX"},
		{"App.java", "// TODO: Java file\nclass App {}\n", "Java"},
		{"main.c", "// TODO: C file\nint main() {}\n", "C"},
		{"main.cpp", "// TODO: C++ file\nint main() {}\n", "C++"},
		{"Program.cs", "// TODO: C# file\nclass Program {}\n", "C#"},
		{"script.rb", "# TODO: Ruby file\nputs 'test'\n", "Ruby"},
		{"index.php", "// TODO: PHP file\n<?php echo 'test'; ?>\n", "PHP"},
		{"script.sh", "# TODO: Bash file\necho 'test'\n", "Bash"},
	}

	// Also create some non-source files that should be ignored
	nonSourceFiles := []string{
		"README.md",
		"data.json",
		"config.yaml",
		"image.png",
		"document.pdf",
	}

	// Create source files
	for _, sf := range sourceFiles {
		path := filepath.Join(tmpDir, sf.name)
		if err := os.WriteFile(path, []byte(sf.content), 0644); err != nil {
			t.Fatal(err)
		}
	}

	// Create non-source files
	for _, name := range nonSourceFiles {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte("content"), 0644); err != nil {
			t.Fatal(err)
		}
	}

	cfg := config.DefaultConfig()
	s := scanner.NewScanner(cfg)

	files, err := s.ScanDir(tmpDir)
	if err != nil {
		t.Fatalf("ScanDir failed: %v", err)
	}

	// Should find all source files but no non-source files
	if len(files) != len(sourceFiles) {
		t.Errorf("Expected %d source files, got %d", len(sourceFiles), len(files))
		t.Logf("Found files:")
		for _, f := range files {
			t.Logf("  %s", f)
		}
	}

	// Verify all source files were found
	foundFiles := make(map[string]bool)
	for _, f := range files {
		basename := filepath.Base(f)
		foundFiles[basename] = true
	}

	for _, sf := range sourceFiles {
		if !foundFiles[sf.name] {
			t.Errorf("Source file %s (%s) was not found", sf.name, sf.lang)
		}
	}

	// Verify no non-source files were found
	for _, f := range files {
		basename := filepath.Base(f)
		for _, nonSource := range nonSourceFiles {
			if basename == nonSource {
				t.Errorf("Non-source file %s should not have been found", nonSource)
			}
		}
	}
}

// Category 12: Determinism and Edge Case Tests

func TestSATDExtractionDeterministic(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "determinism.go")

	content := `package main

// TODO: First task
func alpha() {
	// FIXME: Second issue
}

// HACK: Third workaround
type Beta struct {}

// BUG: Fourth problem
func gamma() {}
`

	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()

	runs := 5
	var results [][]models.TechnicalDebt

	for i := 0; i < runs; i++ {
		items, err := analyzer.AnalyzeFile(testFile)
		if err != nil {
			t.Fatalf("Run %d: AnalyzeFile failed: %v", i+1, err)
		}
		results = append(results, items)
	}

	if len(results[0]) == 0 {
		t.Fatal("Expected debt items but got none")
	}

	for runIdx := 1; runIdx < runs; runIdx++ {
		if len(results[runIdx]) != len(results[0]) {
			t.Errorf("Run %d returned %d items, expected %d items",
				runIdx+1, len(results[runIdx]), len(results[0]))
			continue
		}

		for itemIdx := 0; itemIdx < len(results[0]); itemIdx++ {
			current := results[runIdx][itemIdx]
			baseline := results[0][itemIdx]

			if current.Category != baseline.Category {
				t.Errorf("Run %d, item %d: category mismatch: got %s, want %s",
					runIdx+1, itemIdx, current.Category, baseline.Category)
			}
			if current.Severity != baseline.Severity {
				t.Errorf("Run %d, item %d: severity mismatch: got %s, want %s",
					runIdx+1, itemIdx, current.Severity, baseline.Severity)
			}
			if current.Line != baseline.Line {
				t.Errorf("Run %d, item %d: line mismatch: got %d, want %d",
					runIdx+1, itemIdx, current.Line, baseline.Line)
			}
			if current.Marker != baseline.Marker {
				t.Errorf("Run %d, item %d: marker mismatch: got %s, want %s",
					runIdx+1, itemIdx, current.Marker, baseline.Marker)
			}
		}
	}
}

func TestSATDEmptyInputHandled(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewSATDAnalyzer()

	tests := []struct {
		name    string
		content string
	}{
		{
			name:    "empty_file",
			content: "",
		},
		{
			name:    "only_whitespace",
			content: "   \t   \n   \t   \n",
		},
		{
			name:    "only_newlines",
			content: "\n\n\n\n\n",
		},
		{
			name:    "spaces_and_newlines",
			content: "     \n     \n     \n",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			testFile := filepath.Join(tmpDir, tt.name+".go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Errorf("AnalyzeFile should handle empty input without error: %v", err)
			}

			if len(items) != 0 {
				t.Errorf("Expected no debt items for empty input, got %d items", len(items))
			}
		})
	}
}

func TestSATDMalformedCommentsHandled(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewSATDAnalyzer()

	tests := []struct {
		name       string
		content    string
		expectDebt bool
	}{
		{
			name: "unclosed_block_comment",
			content: `package main
/* TODO: This is an unclosed block comment
func main() {}
`,
			expectDebt: true,
		},
		{
			name: "comment_with_unicode",
			content: `package main
// TODO: Add support for emoji 🚀 and unicode αβγ
func main() {}
`,
			expectDebt: true,
		},
		{
			name: "comment_with_special_chars",
			content: `package main
// FIXME: Handle special chars: !@#$%^&*()[]{}|<>?
func main() {}
`,
			expectDebt: true,
		},
		{
			name: "very_long_single_line",
			content: `package main
// TODO: ` + strings.Repeat("This is a very long comment with repetitive text. ", 50) + `
func main() {}
`,
			expectDebt: true,
		},
		{
			name: "nested_block_comments_simulation",
			content: `package main
/* TODO: Outer comment /* inner text */ still outer */
func main() {}
`,
			expectDebt: true,
		},
		{
			name:       "comment_with_null_bytes_removed",
			content:    "package main\n// HACK: This comment has special handling\nfunc main() {}\n",
			expectDebt: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			testFile := filepath.Join(tmpDir, tt.name+".go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			items, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Errorf("AnalyzeFile should handle malformed comments gracefully: %v", err)
			}

			if tt.expectDebt && len(items) == 0 {
				t.Errorf("Expected debt to be detected despite malformed comment")
			}
		})
	}
}

// Category 13: Integration Tests

func TestSATDFileIntegration(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "integration.go")

	content := `package main

import "fmt"

// TODO: Add proper error handling
func processData(data []byte) error {
	// FIXME: This validation is incomplete
	if len(data) == 0 {
		return fmt.Errorf("empty data")
	}

	// HACK: Using string conversion for performance
	str := string(data)

	// SECURITY: Need to sanitize input before processing
	// BUG: Doesn't handle unicode correctly
	fmt.Println(str)

	return nil
}

// OPTIMIZE: This function is too slow
func slowOperation() {
	// WORKAROUND: Skip validation for now
	for i := 0; i < 1000000; i++ {
		_ = i * 2
	}
}

// TEST: Add unit tests for edge cases
func helperFunc() {
	// NOTE: This is a placeholder implementation
}
`

	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	items, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	expectedDebts := map[string]struct {
		category models.DebtCategory
		severity models.Severity
	}{
		"TODO":       {models.DebtRequirement, models.SeverityLow},
		"FIXME":      {models.DebtDefect, models.SeverityHigh},
		"HACK":       {models.DebtDesign, models.SeverityMedium},
		"SECURITY":   {models.DebtSecurity, models.SeverityCritical},
		"BUG":        {models.DebtDefect, models.SeverityHigh},
		"OPTIMIZE":   {models.DebtPerformance, models.SeverityLow},
		"WORKAROUND": {models.DebtDesign, models.SeverityLow},
		"TEST":       {models.DebtTest, models.SeverityLow},
		"NOTE":       {models.DebtDesign, models.SeverityLow},
	}

	if len(items) != len(expectedDebts) {
		t.Errorf("Expected %d debt items, got %d", len(expectedDebts), len(items))
	}

	foundMarkers := make(map[string]bool)
	for _, item := range items {
		foundMarkers[item.Marker] = true
		expected, ok := expectedDebts[item.Marker]
		if !ok {
			t.Errorf("Unexpected marker found: %s", item.Marker)
			continue
		}

		if item.Category != expected.category {
			t.Errorf("Marker %s: expected category %s, got %s",
				item.Marker, expected.category, item.Category)
		}

		if item.File != testFile {
			t.Errorf("Marker %s: expected file %s, got %s",
				item.Marker, testFile, item.File)
		}

		if item.Line == 0 {
			t.Errorf("Marker %s: line number should not be 0", item.Marker)
		}

		if item.Description == "" {
			t.Errorf("Marker %s: description should not be empty", item.Marker)
		}
	}

	for marker := range expectedDebts {
		if !foundMarkers[marker] {
			t.Errorf("Expected marker %s not found in results", marker)
		}
	}
}

func TestGenerateMetricsFromDebts(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"file1.go": `package main
// TODO: First task
// FIXME: First fix
`,
		"file2.go": `package main
// BUG: Critical bug
// SECURITY: Security issue
// HACK: Quick hack
`,
		"file3.go": `package main
// TODO: Another task
// TODO: Yet another task
`,
	}

	var allFiles []string
	for filename, content := range files {
		fullPath := filepath.Join(tmpDir, filename)
		if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil {
			t.Fatal(err)
		}
		allFiles = append(allFiles, fullPath)
	}

	analyzer := NewSATDAnalyzer()
	analysis, err := analyzer.AnalyzeProject(allFiles)
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.TotalFilesAnalyzed != len(allFiles) {
		t.Errorf("Expected %d files analyzed, got %d",
			len(allFiles), analysis.TotalFilesAnalyzed)
	}

	expectedTotal := 7
	if analysis.Summary.TotalItems != expectedTotal {
		t.Errorf("Expected %d total items, got %d",
			expectedTotal, analysis.Summary.TotalItems)
	}

	expectedByCategory := map[string]int{
		string(models.DebtRequirement): 3,
		string(models.DebtDefect):      2,
		string(models.DebtSecurity):    1,
		string(models.DebtDesign):      1,
	}

	for category, expectedCount := range expectedByCategory {
		actualCount := analysis.Summary.ByCategory[category]
		if actualCount != expectedCount {
			t.Errorf("Category %s: expected %d items, got %d",
				category, expectedCount, actualCount)
		}
	}

	expectedBySeverity := map[string]int{
		string(models.SeverityLow):      3,
		string(models.SeverityHigh):     2,
		string(models.SeverityCritical): 1,
		string(models.SeverityMedium):   1,
	}

	for severity, expectedCount := range expectedBySeverity {
		actualCount := analysis.Summary.BySeverity[severity]
		if actualCount != expectedCount {
			t.Errorf("Severity %s: expected %d items, got %d",
				severity, expectedCount, actualCount)
		}
	}

	if analysis.FilesWithDebt != len(files) {
		t.Errorf("Expected %d files with debt, got %d",
			len(files), analysis.FilesWithDebt)
	}

	for _, item := range analysis.Items {
		if item.Line == 0 {
			t.Error("All debt items should have non-zero line numbers")
		}
		if item.File == "" {
			t.Error("All debt items should have file paths")
		}
		if item.Marker == "" {
			t.Error("All debt items should have markers")
		}
	}
}

// Category 10: Multiline and Nested Comment Tests

func TestSATDMultilineHandling(t *testing.T) {
	tests := []struct {
		name           string
		content        string
		expectedCount  int
		expectedMarker string
	}{
		{
			name: "multiline block comment with TODO",
			content: `/* TODO: this is
   a multiline
   comment */
func main() {}`,
			expectedCount:  1,
			expectedMarker: "TODO",
		},
		{
			name: "multiline block comment with FIXME",
			content: `/* FIXME: critical issue
   needs immediate attention
   affects production */
func process() {}`,
			expectedCount:  1,
			expectedMarker: "FIXME",
		},
		{
			name: "multiple TODOs in separate multiline blocks",
			content: `/* TODO: first issue
   continues here */
func a() {}

/* TODO: second issue
   more details */
func b() {}`,
			expectedCount:  2,
			expectedMarker: "TODO",
		},
		{
			name: "block comment with marker properly formatted",
			content: `/* This is a comment
 * TODO: marker on second line
 * more info */
func main() {}`,
			expectedCount:  1,
			expectedMarker: "TODO",
		},
		{
			name: "Python multiline docstring with TODO",
			content: `"""TODO: implement this
function properly
with better error handling"""
def process():
    pass`,
			expectedCount:  1,
			expectedMarker: "TODO",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			filename := filepath.Join(tmpDir, "test.go")
			if strings.Contains(tt.content, "def ") {
				filename = filepath.Join(tmpDir, "test.py")
			}

			if err := os.WriteFile(filename, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			items, err := analyzer.AnalyzeFile(filename)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) != tt.expectedCount {
				t.Errorf("Expected %d items, got %d", tt.expectedCount, len(items))
			}

			if tt.expectedCount > 0 && len(items) > 0 {
				if items[0].Marker != tt.expectedMarker {
					t.Errorf("Expected marker %q, got %q", tt.expectedMarker, items[0].Marker)
				}
			}
		})
	}
}

func TestSATDNestedComments(t *testing.T) {
	tests := []struct {
		name          string
		content       string
		expectedCount int
		description   string
	}{
		{
			name: "multiple markers in same block comment",
			content: `/* TODO: implement feature
 * FIXME: fix the bug first
 * HACK: temporary workaround */
func main() {}`,
			expectedCount: 3,
			description:   "Should detect all three markers in one block",
		},
		{
			name: "nested line comments with markers",
			content: `// TODO: outer task
// NOTE: this is important
//   FIXME: nested detail
func process() {}`,
			expectedCount: 3,
			description:   "Should detect all markers in consecutive line comments",
		},
		{
			name: "marker in comment within commented code",
			content: `// TODO: refactor this
// func old() {
//   // FIXME: this was broken
// }
func new() {}`,
			expectedCount: 2,
			description:   "Should detect markers even in commented-out code",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			filename := filepath.Join(tmpDir, "test.go")

			if err := os.WriteFile(filename, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			items, err := analyzer.AnalyzeFile(filename)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) != tt.expectedCount {
				t.Errorf("%s: Expected %d items, got %d", tt.description, tt.expectedCount, len(items))
			}
		})
	}
}

func TestWithNestedBlocks(t *testing.T) {
	tests := []struct {
		name          string
		content       string
		expectedCount int
		language      string
	}{
		{
			name: "TODO inside nested function",
			content: `func outer() {
	func inner() {
		// TODO: implement inner logic
		return
	}
}`,
			expectedCount: 1,
			language:      "go",
		},
		{
			name: "FIXME inside closure",
			content: `func process() {
	handler := func() {
		// FIXME: handle errors properly
		doSomething()
	}
}`,
			expectedCount: 1,
			language:      "go",
		},
		{
			name: "multiple TODOs in nested blocks",
			content: `func outer() {
	// TODO: outer task
	if condition {
		// TODO: inner task
		for i := 0; i < 10; i++ {
			// TODO: deepest task
		}
	}
}`,
			expectedCount: 3,
			language:      "go",
		},
		{
			name: "JavaScript nested callback with HACK",
			content: `function process() {
	callback(function() {
		// HACK: temporary fix
		return data;
	});
}`,
			expectedCount: 1,
			language:      "javascript",
		},
		{
			name: "Python nested function with TODO",
			content: `def outer():
    def inner():
        # TODO: implement validation
        pass
    return inner`,
			expectedCount: 1,
			language:      "python",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			ext := languageToExtension(tt.language)
			filename := filepath.Join(tmpDir, "test"+ext)

			if err := os.WriteFile(filename, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			items, err := analyzer.AnalyzeFile(filename)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) != tt.expectedCount {
				t.Errorf("Expected %d items, got %d", tt.expectedCount, len(items))
			}
		})
	}
}

// Category 11: Validation Tests

func TestSATDLineNumbersValid(t *testing.T) {
	content := `package main

// TODO: first task
func a() {}

// FIXME: second task
func b() {}

// HACK: third task
func c() {
	// NOTE: nested task
}
`

	tmpDir := t.TempDir()
	filename := filepath.Join(tmpDir, "test.go")

	if err := os.WriteFile(filename, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	items, err := analyzer.AnalyzeFile(filename)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(items) == 0 {
		t.Fatal("Expected to find SATD items")
	}

	for i, item := range items {
		if item.Line == 0 {
			t.Errorf("Item %d has invalid line number 0", i)
		}

		if item.Line > uint32(len(strings.Split(content, "\n"))) {
			t.Errorf("Item %d has line number %d which exceeds file length", i, item.Line)
		}
	}

	expectedLines := map[string]uint32{
		"TODO":  3,
		"FIXME": 6,
		"HACK":  9,
		"NOTE":  11,
	}

	for _, item := range items {
		expectedLine, exists := expectedLines[item.Marker]
		if exists && item.Line != expectedLine {
			t.Errorf("Marker %s expected at line %d, found at line %d", item.Marker, expectedLine, item.Line)
		}
	}
}

func TestSATDSeverityOrdering(t *testing.T) {
	severities := []models.Severity{
		models.SeverityLow,
		models.SeverityMedium,
		models.SeverityHigh,
		models.SeverityCritical,
	}

	for i := 0; i < len(severities)-1; i++ {
		current := severities[i]
		next := severities[i+1]

		if current.Weight() >= next.Weight() {
			t.Errorf("Severity ordering violation: %s (weight %d) should be less than %s (weight %d)",
				current, current.Weight(), next, next.Weight())
		}
	}

	if models.SeverityCritical.Weight() != 4 {
		t.Errorf("Critical severity should have weight 4, got %d", models.SeverityCritical.Weight())
	}
	if models.SeverityHigh.Weight() != 3 {
		t.Errorf("High severity should have weight 3, got %d", models.SeverityHigh.Weight())
	}
	if models.SeverityMedium.Weight() != 2 {
		t.Errorf("Medium severity should have weight 2, got %d", models.SeverityMedium.Weight())
	}
	if models.SeverityLow.Weight() != 1 {
		t.Errorf("Low severity should have weight 1, got %d", models.SeverityLow.Weight())
	}
}

func TestSATDCategoriesConsistent(t *testing.T) {
	tests := []struct {
		marker           string
		expectedCategory models.DebtCategory
		description      string
	}{
		{
			marker:           "TODO",
			expectedCategory: models.DebtRequirement,
			description:      "TODO should always map to requirement category",
		},
		{
			marker:           "FIXME",
			expectedCategory: models.DebtDefect,
			description:      "FIXME should always map to defect category",
		},
		{
			marker:           "BUG",
			expectedCategory: models.DebtDefect,
			description:      "BUG should always map to defect category",
		},
		{
			marker:           "HACK",
			expectedCategory: models.DebtDesign,
			description:      "HACK should always map to design category",
		},
		{
			marker:           "SECURITY",
			expectedCategory: models.DebtSecurity,
			description:      "SECURITY should always map to security category",
		},
		{
			marker:           "OPTIMIZE",
			expectedCategory: models.DebtPerformance,
			description:      "OPTIMIZE should always map to performance category",
		},
	}

	for _, tt := range tests {
		t.Run(tt.marker, func(t *testing.T) {
			tmpDir := t.TempDir()
			filename := filepath.Join(tmpDir, "test.go")

			content := "// " + tt.marker + ": test comment\nfunc main() {}\n"
			if err := os.WriteFile(filename, []byte(content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer()
			items, err := analyzer.AnalyzeFile(filename)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if len(items) == 0 {
				t.Fatalf("Expected to find SATD item for marker %s", tt.marker)
			}

			if items[0].Category != tt.expectedCategory {
				t.Errorf("%s: Expected category %s, got %s",
					tt.description, tt.expectedCategory, items[0].Category)
			}

			filename2 := filepath.Join(tmpDir, "test2.go")
			if err := os.WriteFile(filename2, []byte(content), 0644); err != nil {
				t.Fatal(err)
			}

			items2, err := analyzer.AnalyzeFile(filename2)
			if err != nil {
				t.Fatalf("Second AnalyzeFile failed: %v", err)
			}

			if len(items2) == 0 {
				t.Fatal("Expected to find SATD item in second file")
			}

			if items[0].Category != items2[0].Category {
				t.Errorf("Category assignment is inconsistent: first=%s, second=%s",
					items[0].Category, items2[0].Category)
			}
		})
	}
}

// ============================================================================
// Strict Mode Tests (PMAT feature)
// ============================================================================

func TestNewSATDAnalyzerStrict(t *testing.T) {
	analyzer := NewSATDAnalyzer(WithSATDStrictMode(true))
	if analyzer == nil {
		t.Fatal("NewSATDAnalyzer with strict mode returned nil")
	}

	// Strict mode should have fewer patterns than default
	defaultAnalyzer := NewSATDAnalyzer()
	if len(analyzer.patterns) >= len(defaultAnalyzer.patterns) {
		t.Errorf("Strict mode should have fewer patterns (%d) than default (%d)",
			len(analyzer.patterns), len(defaultAnalyzer.patterns))
	}

	// Verify strict mode is enabled
	if !analyzer.strictMode {
		t.Error("StrictMode should be true")
	}
}

func TestStrictModePatterns(t *testing.T) {
	patterns := strictSATDPatterns()
	if len(patterns) == 0 {
		t.Fatal("strictSATDPatterns should return patterns")
	}

	// All strict patterns should require colon format
	for _, p := range patterns {
		pattern := p.regex.String()
		if !strings.Contains(pattern, ":") {
			t.Errorf("Strict pattern should require colon: %s", pattern)
		}
	}
}

func TestStrictModeMatchesExplicitMarkers(t *testing.T) {
	tests := []struct {
		name        string
		content     string
		shouldMatch bool
		category    models.DebtCategory
	}{
		{
			name:        "TODO with colon matches",
			content:     "// TODO: implement this feature\n",
			shouldMatch: true,
			category:    models.DebtRequirement,
		},
		{
			name:        "FIXME with colon matches",
			content:     "// FIXME: this crashes sometimes\n",
			shouldMatch: true,
			category:    models.DebtDefect,
		},
		{
			name:        "HACK with colon matches",
			content:     "// HACK: workaround for library bug\n",
			shouldMatch: true,
			category:    models.DebtDesign,
		},
		{
			name:        "XXX with colon matches",
			content:     "// XXX: needs review\n",
			shouldMatch: true,
			category:    models.DebtDesign,
		},
		{
			name:        "BUG with colon matches",
			content:     "// BUG: memory leak here\n",
			shouldMatch: true,
			category:    models.DebtDefect,
		},
		{
			name:        "TODO without colon does not match in strict mode",
			content:     "// TODO implement feature\n",
			shouldMatch: false,
		},
		{
			name:        "FIXME without colon does not match in strict mode",
			content:     "// FIXME broken logic\n",
			shouldMatch: false,
		},
		{
			name:        "Regular comment does not match",
			content:     "// This is a regular comment\n",
			shouldMatch: false,
		},
		{
			name:        "todo lowercase does not match strict",
			content:     "// todo: lowercase marker\n",
			shouldMatch: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.content), 0644); err != nil {
				t.Fatal(err)
			}

			analyzer := NewSATDAnalyzer(WithSATDStrictMode(true))
			debts, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile failed: %v", err)
			}

			if tt.shouldMatch {
				if len(debts) == 0 {
					t.Errorf("Expected strict mode to match: %s", tt.content)
				} else if debts[0].Category != tt.category {
					t.Errorf("Expected category %s, got %s", tt.category, debts[0].Category)
				}
			} else {
				if len(debts) > 0 {
					t.Errorf("Expected strict mode NOT to match: %s (got %d matches)", tt.content, len(debts))
				}
			}
		})
	}
}

func TestStrictModeVsDefaultMode(t *testing.T) {
	content := `// TODO implement without colon
// TODO: implement with colon
// FIXME broken without colon
// FIXME: broken with colon
// Just a regular comment
func main() {}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// Default mode should find more matches
	defaultAnalyzer := NewSATDAnalyzer()
	defaultDebts, err := defaultAnalyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("Default analyzer failed: %v", err)
	}

	// Strict mode should find fewer matches
	strictAnalyzer := NewSATDAnalyzer(WithSATDStrictMode(true))
	strictDebts, err := strictAnalyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("Strict analyzer failed: %v", err)
	}

	if len(strictDebts) >= len(defaultDebts) {
		t.Errorf("Strict mode should find fewer matches (%d) than default (%d)",
			len(strictDebts), len(defaultDebts))
	}

	// Strict mode should only match lines with colons
	if len(strictDebts) != 2 {
		t.Errorf("Strict mode should find exactly 2 matches (TODO: and FIXME:), got %d", len(strictDebts))
	}
}

func TestStrictModeWithOptions(t *testing.T) {
	analyzer := NewSATDAnalyzer(WithSATDStrictMode(true))
	if !analyzer.strictMode {
		t.Error("Analyzer should have StrictMode enabled")
	}

	// Verify it uses strict patterns
	patterns := strictSATDPatterns()
	if len(analyzer.patterns) != len(patterns) {
		t.Errorf("Expected %d strict patterns, got %d", len(patterns), len(analyzer.patterns))
	}
}

// ============================================================================
// Rust Test Block Tracking Tests (PMAT feature)
// ============================================================================

func TestTestBlockTracker_Creation(t *testing.T) {
	tracker := newTestBlockTracker(true)
	if tracker == nil {
		t.Fatal("newTestBlockTracker returned nil")
	}
	if tracker.isInTestBlock() {
		t.Error("Newly created tracker should not be in test block")
	}
}

func TestTestBlockTracker_Disabled(t *testing.T) {
	tracker := newTestBlockTracker(false)

	// Even with #[cfg(test)], disabled tracker should not track
	tracker.updateFromLine("#[cfg(test)]")
	if tracker.isInTestBlock() {
		t.Error("Disabled tracker should not track test blocks")
	}
}

func TestTestBlockTracker_SimpleTestBlock(t *testing.T) {
	tracker := newTestBlockTracker(true)

	// Before #[cfg(test)]
	if tracker.isInTestBlock() {
		t.Error("Should not be in test block initially")
	}

	// Enter test block immediately when #[cfg(test)] is seen
	// (conservative approach to exclude any SATD between attribute and opening brace)
	tracker.updateFromLine("#[cfg(test)]")
	if !tracker.isInTestBlock() {
		t.Error("Should be in test block after #[cfg(test)]")
	}

	tracker.updateFromLine("mod tests {")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block after opening brace")
	}

	// Still in test block
	tracker.updateFromLine("    fn test_something() {")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block")
	}

	tracker.updateFromLine("    }")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block (inner function closed)")
	}

	// Exit test block
	tracker.updateFromLine("}")
	if tracker.isInTestBlock() {
		t.Error("Should exit test block after closing brace")
	}
}

func TestTestBlockTracker_NestedBraces(t *testing.T) {
	tracker := newTestBlockTracker(true)

	tracker.updateFromLine("#[cfg(test)]")
	tracker.updateFromLine("mod tests {")
	if !tracker.isInTestBlock() {
		t.Error("Should be in test block")
	}

	// Nested function
	tracker.updateFromLine("    fn test_nested() {")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block with nested function")
	}

	// Another level of nesting
	tracker.updateFromLine("        if true {")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block with nested if")
	}

	tracker.updateFromLine("        }")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block after closing if")
	}

	tracker.updateFromLine("    }")
	if !tracker.isInTestBlock() {
		t.Error("Should still be in test block after closing function")
	}

	tracker.updateFromLine("}")
	if tracker.isInTestBlock() {
		t.Error("Should exit test block after closing module")
	}
}

func TestRustTestBlockExclusion(t *testing.T) {
	content := `fn production_code() {
    // TODO: important production task
}

#[cfg(test)]
mod tests {
    // TODO: this should be excluded
    fn test_something() {}
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.rs")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// With test block exclusion enabled (default)
	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Should only find the production TODO, not the test TODO
	if len(debts) != 1 {
		t.Errorf("Expected 1 debt item (production only), got %d", len(debts))
		for _, d := range debts {
			t.Logf("  Found: line %d: %s", d.Line, d.Description)
		}
	}

	if len(debts) > 0 && debts[0].Line != 2 {
		t.Errorf("Expected debt on line 2 (production), got line %d", debts[0].Line)
	}
}

func TestRustTestBlockExclusionDisabled(t *testing.T) {
	content := `fn production_code() {
    // TODO: important production task
}

#[cfg(test)]
mod tests {
    // TODO: this should be included
    fn test_something() {}
}
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.rs")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	// With test block exclusion disabled
	analyzer := NewSATDAnalyzer(WithSATDExcludeTestBlocks(false))
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Should find both TODOs
	if len(debts) != 2 {
		t.Errorf("Expected 2 debt items (both), got %d", len(debts))
	}
}

func TestTestBlockTracker_OnlyAffectsRust(t *testing.T) {
	// Test that non-Rust files are not affected by test block tracking
	content := `fn production_code() {
    // TODO: important production task
}

#[cfg(test)]
mod tests {
    // TODO: this is not Rust so should be found
    fn test_something() {}
}
`
	tmpDir := t.TempDir()

	// Go file - should find both
	goFile := filepath.Join(tmpDir, "test.go")
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(goFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Go file should find both TODOs (test block tracking only applies to Rust)
	if len(debts) != 2 {
		t.Errorf("Go file should find 2 debt items, got %d", len(debts))
	}
}

func TestRustTestBlockWithMultipleBlocks(t *testing.T) {
	content := `// TODO: found in global scope

fn production() {
    // TODO: found in production code
}

#[cfg(test)]
mod tests {
    // TODO: excluded in test block
}

// TODO: found after test block

#[cfg(test)]
mod more_tests {
    // TODO: also excluded
}

// TODO: found at end
`
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.rs")
	if err := os.WriteFile(testFile, []byte(content), 0644); err != nil {
		t.Fatal(err)
	}

	analyzer := NewSATDAnalyzer()
	debts, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	// Should find TODOs outside test blocks only (4 total)
	expectedLines := []uint32{1, 4, 12, 19}
	if len(debts) != len(expectedLines) {
		t.Errorf("Expected %d debt items, got %d", len(expectedLines), len(debts))
		for _, d := range debts {
			t.Logf("  Found: line %d: %s", d.Line, d.Description)
		}
	} else {
		for i, debt := range debts {
			if debt.Line != expectedLines[i] {
				t.Errorf("Expected debt on line %d, got line %d", expectedLines[i], debt.Line)
			}
		}
	}
}

// ============================================================================
// AST Context Severity Adjustment Tests (PMAT feature)
// Matches PMAT's test_adjust_severity_* tests
// ============================================================================

func TestAstNodeType_Constants(t *testing.T) {
	// Verify all node types are distinct
	nodeTypes := []AstNodeType{
		AstNodeRegular,
		AstNodeSecurityFunction,
		AstNodeDataValidation,
		AstNodeTestFunction,
		AstNodeMockImplementation,
	}

	seen := make(map[AstNodeType]bool)
	for _, nt := range nodeTypes {
		if seen[nt] {
			t.Errorf("Duplicate AstNodeType value: %d", nt)
		}
		seen[nt] = true
	}

	if len(nodeTypes) != 5 {
		t.Errorf("Expected 5 AstNodeType values, got %d", len(nodeTypes))
	}
}

func TestAdjustSeverityWithContext_SecurityFunction(t *testing.T) {
	// PMAT: test_adjust_severity_security_function_escalates
	// Security functions should escalate severity
	analyzer := NewSATDAnalyzer()

	ctx := &AstContext{
		NodeType:       AstNodeSecurityFunction,
		ParentFunction: "validate_auth",
		Complexity:     5,
		SiblingsCount:  3,
		NestingDepth:   2,
	}

	tests := []struct {
		input    models.Severity
		expected models.Severity
	}{
		{models.SeverityLow, models.SeverityMedium},
		{models.SeverityMedium, models.SeverityHigh},
		{models.SeverityHigh, models.SeverityCritical},
		{models.SeverityCritical, models.SeverityCritical}, // Already max
	}

	for _, tt := range tests {
		result := analyzer.AdjustSeverityWithContext(tt.input, ctx)
		if result != tt.expected {
			t.Errorf("SecurityFunction: AdjustSeverityWithContext(%s) = %s, expected %s",
				tt.input, result, tt.expected)
		}
	}
}

func TestAdjustSeverityWithContext_DataValidation(t *testing.T) {
	// Data validation functions should escalate severity (same as security)
	analyzer := NewSATDAnalyzer()

	ctx := &AstContext{
		NodeType:       AstNodeDataValidation,
		ParentFunction: "check_data",
		Complexity:     5,
		SiblingsCount:  1,
		NestingDepth:   2,
	}

	result := analyzer.AdjustSeverityWithContext(models.SeverityMedium, ctx)
	if result != models.SeverityHigh {
		t.Errorf("DataValidation: AdjustSeverityWithContext(Medium) = %s, expected High", result)
	}
}

func TestAdjustSeverityWithContext_TestFunction(t *testing.T) {
	// PMAT: test_adjust_severity_test_function_reduces
	// Test functions should reduce severity
	analyzer := NewSATDAnalyzer()

	ctx := &AstContext{
		NodeType:       AstNodeTestFunction,
		ParentFunction: "test_something",
		Complexity:     5,
		SiblingsCount:  3,
		NestingDepth:   2,
	}

	tests := []struct {
		input    models.Severity
		expected models.Severity
	}{
		{models.SeverityCritical, models.SeverityHigh},
		{models.SeverityHigh, models.SeverityMedium},
		{models.SeverityMedium, models.SeverityLow},
		{models.SeverityLow, models.SeverityLow}, // Already min
	}

	for _, tt := range tests {
		result := analyzer.AdjustSeverityWithContext(tt.input, ctx)
		if result != tt.expected {
			t.Errorf("TestFunction: AdjustSeverityWithContext(%s) = %s, expected %s",
				tt.input, result, tt.expected)
		}
	}
}

func TestAdjustSeverityWithContext_MockImplementation(t *testing.T) {
	// Mock implementations should reduce severity (same as test)
	analyzer := NewSATDAnalyzer()

	ctx := &AstContext{
		NodeType:       AstNodeMockImplementation,
		ParentFunction: "mock_service",
		Complexity:     3,
	}

	result := analyzer.AdjustSeverityWithContext(models.SeverityHigh, ctx)
	if result != models.SeverityMedium {
		t.Errorf("MockImplementation: AdjustSeverityWithContext(High) = %s, expected Medium", result)
	}
}

func TestAdjustSeverityWithContext_HighComplexity(t *testing.T) {
	// PMAT: test_adjust_severity_high_complexity_escalates
	// High complexity (>20) should escalate severity for regular nodes
	analyzer := NewSATDAnalyzer()

	ctx := &AstContext{
		NodeType:       AstNodeRegular,
		ParentFunction: "process_data",
		Complexity:     25, // High complexity > 20
		SiblingsCount:  10,
		NestingDepth:   5,
	}

	result := analyzer.AdjustSeverityWithContext(models.SeverityLow, ctx)
	if result != models.SeverityMedium {
		t.Errorf("HighComplexity: AdjustSeverityWithContext(Low) = %s, expected Medium", result)
	}

	result = analyzer.AdjustSeverityWithContext(models.SeverityMedium, ctx)
	if result != models.SeverityHigh {
		t.Errorf("HighComplexity: AdjustSeverityWithContext(Medium) = %s, expected High", result)
	}
}

func TestAdjustSeverityWithContext_RegularUnchanged(t *testing.T) {
	// PMAT: test_adjust_severity_regular_unchanged
	// Regular context with low complexity should not change severity
	analyzer := NewSATDAnalyzer()

	ctx := &AstContext{
		NodeType:       AstNodeRegular,
		ParentFunction: "helper",
		Complexity:     5, // Low complexity
		SiblingsCount:  3,
		NestingDepth:   1,
	}

	for _, sev := range []models.Severity{
		models.SeverityLow,
		models.SeverityMedium,
		models.SeverityHigh,
		models.SeverityCritical,
	} {
		result := analyzer.AdjustSeverityWithContext(sev, ctx)
		if result != sev {
			t.Errorf("Regular: AdjustSeverityWithContext(%s) = %s, expected unchanged", sev, result)
		}
	}
}

func TestAdjustSeverityWithContext_ComplexityThreshold(t *testing.T) {
	// Test the exact threshold of complexity = 20
	analyzer := NewSATDAnalyzer()

	// Complexity exactly 20 should NOT escalate
	ctx20 := &AstContext{NodeType: AstNodeRegular, Complexity: 20}
	result := analyzer.AdjustSeverityWithContext(models.SeverityLow, ctx20)
	if result != models.SeverityLow {
		t.Errorf("Complexity=20 should not escalate, got %s", result)
	}

	// Complexity 21 should escalate
	ctx21 := &AstContext{NodeType: AstNodeRegular, Complexity: 21}
	result = analyzer.AdjustSeverityWithContext(models.SeverityLow, ctx21)
	if result != models.SeverityMedium {
		t.Errorf("Complexity=21 should escalate Low to Medium, got %s", result)
	}
}

func TestAstContext_DefaultValues(t *testing.T) {
	// Zero-value AstContext should be valid and not crash
	ctx := &AstContext{}

	analyzer := NewSATDAnalyzer()
	result := analyzer.AdjustSeverityWithContext(models.SeverityMedium, ctx)

	// Zero NodeType is AstNodeRegular, zero Complexity is < 20, so no change
	if result != models.SeverityMedium {
		t.Errorf("Default AstContext should not change severity, got %s", result)
	}
}
