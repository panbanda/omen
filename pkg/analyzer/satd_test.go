package analyzer

import (
	"os"
	"path/filepath"
	"testing"

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
	progressCount := 0
	progressFunc := func() {
		progressCount++
	}

	analysis, err := analyzer.AnalyzeProjectWithProgress(files, progressFunc)

	if err != nil {
		t.Fatalf("AnalyzeProjectWithProgress failed: %v", err)
	}

	if analysis.Summary.TotalItems != 5 {
		t.Errorf("Expected 5 items, got %d", analysis.Summary.TotalItems)
	}

	if progressCount != 5 {
		t.Errorf("Expected progress callback to be called 5 times, got %d", progressCount)
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
