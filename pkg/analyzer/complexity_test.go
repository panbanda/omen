package analyzer

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

func TestNewComplexityAnalyzer(t *testing.T) {
	analyzer := NewComplexityAnalyzer()
	if analyzer == nil {
		t.Fatal("NewComplexityAnalyzer() returned nil")
	}
	if analyzer.parser == nil {
		t.Error("analyzer.parser is nil")
	}
	analyzer.Close()
}

func TestAnalyzeFile_MultiLanguage(t *testing.T) {
	tests := []struct {
		name              string
		language          string
		fileExt           string
		code              string
		wantFunctionCount int
		wantMinCyclomatic uint32
		wantMinCognitive  uint32
	}{
		{
			name:     "Go simple function",
			language: "go",
			fileExt:  ".go",
			code: `package main

func simple() int {
	return 42
}`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:     "Go with if statement",
			language: "go",
			fileExt:  ".go",
			code: `package main

func withIf(x int) int {
	if x > 0 {
		return x
	}
	return 0
}`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 2,
			wantMinCognitive:  1,
		},
		{
			name:     "Python simple function",
			language: "python",
			fileExt:  ".py",
			code: `def simple():
    return 42`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:     "Python with if statement",
			language: "python",
			fileExt:  ".py",
			code: `def with_if(x):
    if x > 0:
        return x
    return 0`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 2,
			wantMinCognitive:  1,
		},
		{
			name:     "Rust simple function",
			language: "rust",
			fileExt:  ".rs",
			code: `fn simple() -> i32 {
    42
}`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:     "Rust with match",
			language: "rust",
			fileExt:  ".rs",
			code: `fn with_match(x: i32) -> i32 {
    match x {
        0 => 0,
        _ => x,
    }
}`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 2,
			wantMinCognitive:  1,
		},
		{
			name:     "TypeScript arrow function",
			language: "typescript",
			fileExt:  ".ts",
			code: `const simple = (): number => {
    return 42;
};`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:     "JavaScript with if",
			language: "javascript",
			fileExt:  ".js",
			code: `const withIf = (x) => {
    if (x > 0) {
        return x;
    }
    return 0;
};`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 2,
			wantMinCognitive:  1,
		},
		{
			name:     "Java simple method",
			language: "java",
			fileExt:  ".java",
			code: `class Test {
    public int simple() {
        return 42;
    }
}`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:     "C simple function",
			language: "c",
			fileExt:  ".c",
			code: `int simple() {
    return 42;
}`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:     "Ruby simple method",
			language: "ruby",
			fileExt:  ".rb",
			code: `def simple
  42
end`,
			wantFunctionCount: 1,
			wantMinCyclomatic: 1,
			wantMinCognitive:  0,
		},
		{
			name:              "Empty file",
			language:          "go",
			fileExt:           ".go",
			code:              `package main`,
			wantFunctionCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test"+tt.fileExt)
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewComplexityAnalyzer()
			defer analyzer.Close()

			result, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile() error = %v", err)
			}

			if result == nil {
				t.Fatal("AnalyzeFile() returned nil result")
			}

			if result.Language != tt.language {
				t.Errorf("Language = %v, want %v", result.Language, tt.language)
			}

			if len(result.Functions) != tt.wantFunctionCount {
				t.Errorf("Function count = %v, want %v", len(result.Functions), tt.wantFunctionCount)
			}

			if tt.wantFunctionCount > 0 {
				fn := result.Functions[0]
				if fn.Metrics.Cyclomatic < tt.wantMinCyclomatic {
					t.Errorf("Cyclomatic = %v, want >= %v", fn.Metrics.Cyclomatic, tt.wantMinCyclomatic)
				}
				if fn.Metrics.Cognitive < tt.wantMinCognitive {
					t.Errorf("Cognitive = %v, want >= %v", fn.Metrics.Cognitive, tt.wantMinCognitive)
				}
			}
		})
	}
}

func TestAnalyzeFile_ComplexControlFlow(t *testing.T) {
	tests := []struct {
		name             string
		code             string
		wantCyclomatic   uint32
		wantMinCognitive uint32
		wantMaxNesting   int
	}{
		{
			name: "Multiple if statements",
			code: `package main

func multiIf(x, y, z int) int {
	if x > 0 {
		return x
	}
	if y > 0 {
		return y
	}
	if z > 0 {
		return z
	}
	return 0
}`,
			wantCyclomatic:   4,
			wantMinCognitive: 3,
			wantMaxNesting:   2,
		},
		{
			name: "Nested if statements",
			code: `package main

func nestedIf(x, y int) int {
	if x > 0 {
		if y > 0 {
			return x + y
		}
		return x
	}
	return 0
}`,
			wantCyclomatic:   3,
			wantMinCognitive: 3,
			wantMaxNesting:   4,
		},
		{
			name: "For loops",
			code: `package main

func forLoops(n int) int {
	sum := 0
	for i := 0; i < 10; i++ {
		sum += i
	}
	return sum
}`,
			wantCyclomatic:   2,
			wantMinCognitive: 1,
			wantMaxNesting:   2,
		},
		{
			name: "Switch statement",
			code: `package main

func switchCase(x int) int {
	switch x {
	case 1:
		return 1
	case 2:
		return 2
	case 3:
		return 3
	default:
		return 0
	}
}`,
			wantCyclomatic:   2,
			wantMinCognitive: 0,
			wantMaxNesting:   1,
		},
		{
			name: "Logical operators",
			code: `package main

func logicalOps(x, y, z int) bool {
	return x > 0 && y > 0 || z > 0
}`,
			wantCyclomatic:   3,
			wantMinCognitive: 0,
			wantMaxNesting:   0,
		},
		{
			name: "Deeply nested",
			code: `package main

func deepNesting(a, b, c, d int) int {
	if a > 0 {
		if b > 0 {
			if c > 0 {
				if d > 0 {
					return a + b + c + d
				}
			}
		}
	}
	return 0
}`,
			wantCyclomatic:   5,
			wantMinCognitive: 10,
			wantMaxNesting:   8,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewComplexityAnalyzer()
			defer analyzer.Close()

			result, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile() error = %v", err)
			}

			if len(result.Functions) == 0 {
				t.Fatal("No functions found")
			}

			fn := result.Functions[0]
			if fn.Metrics.Cyclomatic != tt.wantCyclomatic {
				t.Errorf("Cyclomatic = %v, want %v", fn.Metrics.Cyclomatic, tt.wantCyclomatic)
			}
			if fn.Metrics.Cognitive < tt.wantMinCognitive {
				t.Errorf("Cognitive = %v, want >= %v", fn.Metrics.Cognitive, tt.wantMinCognitive)
			}
			if fn.Metrics.MaxNesting != tt.wantMaxNesting {
				t.Errorf("MaxNesting = %v, want %v", fn.Metrics.MaxNesting, tt.wantMaxNesting)
			}
		})
	}
}

func TestComplexityAnalyzer_AnalyzeProject(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"simple.go": `package main

func simple() int {
	return 42
}`,
		"complex.go": `package main

func complex(x, y int) int {
	if x > 0 {
		if y > 0 {
			return x + y
		}
		return x
	}
	return 0
}`,
		"loops.go": `package main

func loops(n int) int {
	sum := 0
	for i := 0; i < 10; i++ {
		sum += i
	}
	return sum
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

	analyzer := NewComplexityAnalyzer()
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject(filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if result.Summary.TotalFiles != 3 {
		t.Errorf("TotalFiles = %v, want 3", result.Summary.TotalFiles)
	}

	if result.Summary.TotalFunctions != 3 {
		t.Errorf("TotalFunctions = %v, want 3", result.Summary.TotalFunctions)
	}

	if result.Summary.AvgCyclomatic == 0 {
		t.Error("AvgCyclomatic should not be 0")
	}

	if result.Summary.MaxCyclomatic == 0 {
		t.Error("MaxCyclomatic should not be 0")
	}

	if result.Summary.P50Cyclomatic == 0 {
		t.Error("P50Cyclomatic should not be 0")
	}
}

func TestCountDecisionPoints(t *testing.T) {
	tests := []struct {
		name      string
		lang      parser.Language
		code      string
		wantCount uint32
	}{
		{
			name: "Go if statement",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {}
}`,
			wantCount: 1,
		},
		{
			name: "Go multiple ifs",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {}
	if false {}
}`,
			wantCount: 2,
		},
		{
			name: "Go for loop",
			lang: parser.LangGo,
			code: `package main
func test() {
	for i := 0; i < 10; i++ {}
}`,
			wantCount: 1,
		},
		{
			name: "Go switch",
			lang: parser.LangGo,
			code: `package main
func test(x int) {
	switch x {
	case 1:
	case 2:
	case 3:
	}
}`,
			wantCount: 1,
		},
		{
			name: "Go logical operators",
			lang: parser.LangGo,
			code: `package main
func test(a, b, c bool) bool {
	return a && b || c
}`,
			wantCount: 2,
		},
		{
			name: "Python if statement",
			lang: parser.LangPython,
			code: `def test():
    if True:
        pass`,
			wantCount: 1,
		},
		{
			name: "Python elif",
			lang: parser.LangPython,
			code: `def test(x):
    if x > 0:
        pass
    elif x < 0:
        pass`,
			wantCount: 2,
		},
		{
			name: "Rust match",
			lang: parser.LangRust,
			code: `fn test(x: i32) {
    match x {
        0 => {},
        1 => {},
        _ => {},
    }
}`,
			wantCount: 1,
		},
		{
			name:      "Empty function",
			lang:      parser.LangGo,
			code:      `package main\nfunc test() {}`,
			wantCount: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), tt.lang, "test")
			if err != nil {
				t.Fatalf("Parse() error = %v", err)
			}

			root := result.Tree.RootNode()
			count := countDecisionPoints(root, result.Source, tt.lang)

			if count != tt.wantCount {
				t.Errorf("countDecisionPoints() = %v, want %v", count, tt.wantCount)
			}
		})
	}
}

func TestCalculateCognitiveComplexity(t *testing.T) {
	tests := []struct {
		name           string
		lang           parser.Language
		code           string
		wantComplexity uint32
	}{
		{
			name: "Simple if",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {}
}`,
			wantComplexity: 1,
		},
		{
			name: "Nested if",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {
		if true {}
	}
}`,
			wantComplexity: 3,
		},
		{
			name: "Deeply nested",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {
		if true {
			if true {}
		}
	}
}`,
			wantComplexity: 6,
		},
		{
			name: "Multiple same level",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {}
	if true {}
	if true {}
}`,
			wantComplexity: 3,
		},
		{
			name: "For loop",
			lang: parser.LangGo,
			code: `package main
func test() {
	for i := 0; i < 10; i++ {}
}`,
			wantComplexity: 1,
		},
		{
			name: "Nested for in if",
			lang: parser.LangGo,
			code: `package main
func test() {
	if true {
		for i := 0; i < 10; i++ {}
	}
}`,
			wantComplexity: 3,
		},
		{
			name:           "Empty function",
			lang:           parser.LangGo,
			code:           `package main\nfunc test() {}`,
			wantComplexity: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), tt.lang, "test")
			if err != nil {
				t.Fatalf("Parse() error = %v", err)
			}

			root := result.Tree.RootNode()
			complexity := calculateCognitiveComplexity(root, result.Source, tt.lang, 0)

			if complexity != tt.wantComplexity {
				t.Errorf("calculateCognitiveComplexity() = %v, want %v", complexity, tt.wantComplexity)
			}
		})
	}
}

func TestCalculateMaxNesting(t *testing.T) {
	tests := []struct {
		name        string
		code        string
		wantNesting int
	}{
		{
			name: "No nesting",
			code: `package main
func test() {
	x := 1
}`,
			wantNesting: 2,
		},
		{
			name: "Single if",
			code: `package main
func test() {
	if true {
		x := 1
	}
}`,
			wantNesting: 4,
		},
		{
			name: "Nested if",
			code: `package main
func test() {
	if true {
		if true {
			x := 1
		}
	}
}`,
			wantNesting: 6,
		},
		{
			name: "Triple nested",
			code: `package main
func test() {
	if true {
		if true {
			if true {
				x := 1
			}
		}
	}
}`,
			wantNesting: 8,
		},
		{
			name: "For in if",
			code: `package main
func test() {
	if true {
		for i := 0; i < 10; i++ {
			x := 1
		}
	}
}`,
			wantNesting: 6,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangGo, "test")
			if err != nil {
				t.Fatalf("Parse() error = %v", err)
			}

			root := result.Tree.RootNode()
			nesting := calculateMaxNesting(root, result.Source, 0)

			if nesting != tt.wantNesting {
				t.Errorf("calculateMaxNesting() = %v, want %v", nesting, tt.wantNesting)
			}
		})
	}
}

func TestPercentile(t *testing.T) {
	tests := []struct {
		name   string
		values []uint32
		p      int
		want   uint32
	}{
		{
			name:   "Empty slice",
			values: []uint32{},
			p:      50,
			want:   0,
		},
		{
			name:   "Single value",
			values: []uint32{5},
			p:      50,
			want:   5,
		},
		{
			name:   "P50 of sorted values",
			values: []uint32{1, 2, 3, 4, 5},
			p:      50,
			want:   3,
		},
		{
			name:   "P95 of sorted values",
			values: []uint32{1, 2, 3, 4, 5, 6, 7, 8, 9, 10},
			p:      95,
			want:   10,
		},
		{
			name:   "P0 should return first",
			values: []uint32{1, 2, 3, 4, 5},
			p:      0,
			want:   1,
		},
		{
			name:   "P100 should return last",
			values: []uint32{1, 2, 3, 4, 5},
			p:      100,
			want:   5,
		},
		{
			name:   "Large dataset P50",
			values: []uint32{1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10},
			p:      50,
			want:   6,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := percentile(tt.values, tt.p)
			if got != tt.want {
				t.Errorf("percentile() = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestComplexityAnalyzer_EdgeCases(t *testing.T) {
	tests := []struct {
		name    string
		code    string
		wantErr bool
	}{
		{
			name:    "Valid empty package",
			code:    `package main`,
			wantErr: false,
		},
		{
			name: "Function with no body",
			code: `package main
type T interface {
	Method()
}`,
			wantErr: false,
		},
		{
			name: "Very large function",
			code: `package main
func large() {
` + generateLargeFunction(100) + `
}`,
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewComplexityAnalyzer()
			defer analyzer.Close()

			_, err := analyzer.AnalyzeFile(testFile)
			if (err != nil) != tt.wantErr {
				t.Errorf("AnalyzeFile() error = %v, wantErr %v", err, tt.wantErr)
			}
		})
	}
}

func TestAnalyzeFile_InvalidFile(t *testing.T) {
	analyzer := NewComplexityAnalyzer()
	defer analyzer.Close()

	_, err := analyzer.AnalyzeFile("/nonexistent/file.go")
	if err == nil {
		t.Error("AnalyzeFile() should return error for nonexistent file")
	}
}

func TestAnalyzeFile_UnsupportedLanguage(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.xyz")
	if err := os.WriteFile(testFile, []byte("invalid"), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewComplexityAnalyzer()
	defer analyzer.Close()

	_, err := analyzer.AnalyzeFile(testFile)
	if err == nil {
		t.Error("AnalyzeFile() should return error for unsupported language")
	}
}

func TestMakeSet(t *testing.T) {
	tests := []struct {
		name  string
		input []string
		want  map[string]bool
	}{
		{
			name:  "Empty slice",
			input: []string{},
			want:  map[string]bool{},
		},
		{
			name:  "Single item",
			input: []string{"if_statement"},
			want:  map[string]bool{"if_statement": true},
		},
		{
			name:  "Multiple items",
			input: []string{"if_statement", "for_statement", "while_statement"},
			want: map[string]bool{
				"if_statement":    true,
				"for_statement":   true,
				"while_statement": true,
			},
		},
		{
			name:  "Duplicate items",
			input: []string{"if_statement", "if_statement"},
			want:  map[string]bool{"if_statement": true},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := makeSet(tt.input)
			if len(got) != len(tt.want) {
				t.Errorf("makeSet() length = %v, want %v", len(got), len(tt.want))
			}
			for k, v := range tt.want {
				if got[k] != v {
					t.Errorf("makeSet()[%q] = %v, want %v", k, got[k], v)
				}
			}
		})
	}
}

func TestGetOperator(t *testing.T) {
	tests := []struct {
		name string
		code string
		want string
	}{
		{
			name: "Logical AND",
			code: `package main
func test() bool {
	return true && false
}`,
			want: "&&",
		},
		{
			name: "Logical OR",
			code: `package main
func test() bool {
	return true || false
}`,
			want: "||",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangGo, "test")
			if err != nil {
				t.Fatalf("Parse() error = %v", err)
			}

			var foundOp string
			parser.WalkTyped(result.Tree.RootNode(), result.Source, func(n *sitter.Node, nodeType string, src []byte) bool {
				if nodeType == "binary_expression" {
					foundOp = getOperator(n, src)
					return false
				}
				return true
			})

			if foundOp != tt.want {
				t.Errorf("getOperator() = %v, want %v", foundOp, tt.want)
			}
		})
	}
}

func TestGetDecisionNodeTypes(t *testing.T) {
	tests := []struct {
		name         string
		lang         parser.Language
		wantContains []string
	}{
		{
			name: "Go",
			lang: parser.LangGo,
			wantContains: []string{
				"if_statement",
				"for_statement",
				"select_statement",
				"type_switch_statement",
			},
		},
		{
			name: "Python",
			lang: parser.LangPython,
			wantContains: []string{
				"if_statement",
				"elif_clause",
				"except_clause",
				"with_statement",
			},
		},
		{
			name: "Rust",
			lang: parser.LangRust,
			wantContains: []string{
				"if_expression",
				"match_expression",
				"loop_expression",
			},
		},
		{
			name: "Ruby",
			lang: parser.LangRuby,
			wantContains: []string{
				"if",
				"elsif",
				"unless",
				"case",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			types := getDecisionNodeTypes(tt.lang)
			typeSet := makeSet(types)

			for _, want := range tt.wantContains {
				if !typeSet[want] {
					t.Errorf("getDecisionNodeTypes(%v) missing %q", tt.lang, want)
				}
			}
		})
	}
}

func TestGetCognitiveNodeTypes(t *testing.T) {
	tests := []struct {
		name             string
		lang             parser.Language
		wantNestingTypes []string
		wantFlatTypes    []string
	}{
		{
			name: "Go",
			lang: parser.LangGo,
			wantNestingTypes: []string{
				"if_statement",
				"while_statement",
				"for_statement",
			},
			wantFlatTypes: []string{
				"else_clause",
				"break_statement",
				"continue_statement",
			},
		},
		{
			name: "Ruby",
			lang: parser.LangRuby,
			wantNestingTypes: []string{
				"if",
				"unless",
				"while",
				"for",
			},
			wantFlatTypes: []string{
				"elsif",
				"else",
				"break",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			types := getCognitiveNodeTypes(tt.lang)

			nestingFound := make(map[string]bool)
			flatFound := make(map[string]bool)

			for _, ct := range types {
				if ct.incrementsNesting {
					nestingFound[ct.nodeType] = true
				} else {
					flatFound[ct.nodeType] = true
				}
			}

			for _, want := range tt.wantNestingTypes {
				if !nestingFound[want] {
					t.Errorf("getCognitiveNodeTypes(%v) missing nesting type %q", tt.lang, want)
				}
			}

			for _, want := range tt.wantFlatTypes {
				if !flatFound[want] {
					t.Errorf("getCognitiveNodeTypes(%v) missing flat type %q", tt.lang, want)
				}
			}
		})
	}
}

func TestComplexityMetrics_Aggregation(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

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

func complex(x, y, z int) int {
	if x > 0 {
		if y > 0 {
			if z > 0 {
				return x + y + z
			}
			return x + y
		}
		return x
	}
	return 0
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewComplexityAnalyzer()
	defer analyzer.Close()

	result, err := analyzer.AnalyzeFile(testFile)
	if err != nil {
		t.Fatalf("AnalyzeFile() error = %v", err)
	}

	if len(result.Functions) != 3 {
		t.Fatalf("Expected 3 functions, got %d", len(result.Functions))
	}

	var totalCyc, totalCog uint32
	for _, fn := range result.Functions {
		totalCyc += fn.Metrics.Cyclomatic
		totalCog += fn.Metrics.Cognitive
	}

	if result.TotalCyclomatic != totalCyc {
		t.Errorf("TotalCyclomatic = %v, want %v", result.TotalCyclomatic, totalCyc)
	}

	if result.TotalCognitive != totalCog {
		t.Errorf("TotalCognitive = %v, want %v", result.TotalCognitive, totalCog)
	}

	expectedAvgCyc := float64(totalCyc) / float64(len(result.Functions))
	if result.AvgCyclomatic != expectedAvgCyc {
		t.Errorf("AvgCyclomatic = %v, want %v", result.AvgCyclomatic, expectedAvgCyc)
	}
}

func TestAnalyzeFunctionComplexity_NoBody(t *testing.T) {
	code := `package main

type T interface {
	Method()
}`

	p := parser.New()
	defer p.Close()

	result, err := p.Parse([]byte(code), parser.LangGo, "test.go")
	if err != nil {
		t.Fatalf("Parse() error = %v", err)
	}

	functions := parser.GetFunctions(result)
	if len(functions) == 0 {
		t.Skip("No interface methods detected by parser")
	}

	for _, fn := range functions {
		if fn.Body == nil {
			fc := analyzeFunctionComplexity(fn, result)
			if fc.Metrics.Cyclomatic != 1 {
				t.Errorf("Function with no body should have cyclomatic = 1, got %d", fc.Metrics.Cyclomatic)
			}
		}
	}
}

func generateLargeFunction(stmtCount int) string {
	var builder strings.Builder
	for i := 0; i < stmtCount; i++ {
		builder.WriteString(fmt.Sprintf("\tx%d := %d\n", i, i))
	}
	return builder.String()
}

func BenchmarkAnalyzeFile(b *testing.B) {
	tmpDir := b.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

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
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		b.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewComplexityAnalyzer()
	defer analyzer.Close()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := analyzer.AnalyzeFile(testFile)
		if err != nil {
			b.Fatalf("AnalyzeFile() error = %v", err)
		}
	}
}

func BenchmarkCountDecisionPoints(b *testing.B) {
	code := `package main
func test(x, y, z int) int {
	if x > 0 && y > 0 || z > 0 {
		for i := 0; i < 10; i++ {
			if i%2 == 0 {
				x += i
			}
		}
		return x
	}
	return 0
}`

	p := parser.New()
	defer p.Close()

	result, err := p.Parse([]byte(code), parser.LangGo, "test")
	if err != nil {
		b.Fatalf("Parse() error = %v", err)
	}

	root := result.Tree.RootNode()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = countDecisionPoints(root, result.Source, parser.LangGo)
	}
}

func BenchmarkCalculateCognitiveComplexity(b *testing.B) {
	code := `package main
func test() {
	if true {
		if true {
			if true {
				for i := 0; i < 10; i++ {
					if i%2 == 0 {
						x := i
					}
				}
			}
		}
	}
}`

	p := parser.New()
	defer p.Close()

	result, err := p.Parse([]byte(code), parser.LangGo, "test")
	if err != nil {
		b.Fatalf("Parse() error = %v", err)
	}

	root := result.Tree.RootNode()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = calculateCognitiveComplexity(root, result.Source, parser.LangGo, 0)
	}
}

func BenchmarkCalculateMaxNesting(b *testing.B) {
	code := `package main
func test() {
	if true {
		if true {
			if true {
				if true {
					if true {
						x := 1
					}
				}
			}
		}
	}
}`

	p := parser.New()
	defer p.Close()

	result, err := p.Parse([]byte(code), parser.LangGo, "test")
	if err != nil {
		b.Fatalf("Parse() error = %v", err)
	}

	root := result.Tree.RootNode()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = calculateMaxNesting(root, result.Source, 0)
	}
}
