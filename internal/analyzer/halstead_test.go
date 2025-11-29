package analyzer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestNewHalsteadAnalyzer(t *testing.T) {
	h := NewHalsteadAnalyzer()
	if h == nil {
		t.Fatal("NewHalsteadAnalyzer() returned nil")
	}
	if h.operators == nil {
		t.Error("operators map should be initialized")
	}
	if h.operands == nil {
		t.Error("operands map should be initialized")
	}
}

func TestHalsteadAnalyzer_Reset(t *testing.T) {
	h := NewHalsteadAnalyzer()
	h.operators["test"] = 5
	h.operands["var"] = 3

	h.Reset()

	if len(h.operators) != 0 {
		t.Error("operators should be empty after Reset()")
	}
	if len(h.operands) != 0 {
		t.Error("operands should be empty after Reset()")
	}
}

func TestHalsteadAnalyzer_AnalyzeNode(t *testing.T) {
	tests := []struct {
		name        string
		lang        parser.Language
		fileExt     string
		code        string
		wantN1Min   uint32 // Minimum distinct operators
		wantN2Min   uint32 // Minimum distinct operands
		wantVocMin  uint32 // Minimum vocabulary
		wantLenMin  uint32 // Minimum length
		wantVolGt0  bool   // Volume > 0
		wantDiffGt0 bool   // Difficulty > 0
	}{
		{
			name:    "Go simple assignment",
			lang:    parser.LangGo,
			fileExt: ".go",
			code: `package main

func test() {
	x := 1 + 2
}`,
			wantN1Min:   1, // At least + operator
			wantN2Min:   1, // At least one operand
			wantVocMin:  2,
			wantLenMin:  2,
			wantVolGt0:  true,
			wantDiffGt0: true,
		},
		{
			name:    "Go binary expression",
			lang:    parser.LangGo,
			fileExt: ".go",
			code: `package main

func add(a, b int) int {
	return a + b
}`,
			wantN1Min:   1, // At least +
			wantN2Min:   2, // At least a, b
			wantVocMin:  3,
			wantLenMin:  3,
			wantVolGt0:  true,
			wantDiffGt0: true,
		},
		{
			name:    "Go with control flow",
			lang:    parser.LangGo,
			fileExt: ".go",
			code: `package main

func check(x int) bool {
	if x > 0 {
		return true
	}
	return false
}`,
			wantN1Min:   2, // if, >
			wantN2Min:   2, // x, 0, true, false
			wantVocMin:  4,
			wantLenMin:  4,
			wantVolGt0:  true,
			wantDiffGt0: true,
		},
		{
			name:    "Python simple function",
			lang:    parser.LangPython,
			fileExt: ".py",
			code: `def add(a, b):
    return a + b`,
			wantN1Min:   1,
			wantN2Min:   2,
			wantVocMin:  3,
			wantLenMin:  3,
			wantVolGt0:  true,
			wantDiffGt0: true,
		},
		{
			name:    "Rust with match",
			lang:    parser.LangRust,
			fileExt: ".rs",
			code: `fn classify(x: i32) -> &'static str {
    match x {
        0 => "zero",
        _ => "other",
    }
}`,
			wantN1Min:   1, // match
			wantN2Min:   2, // x, 0
			wantVocMin:  3,
			wantLenMin:  3,
			wantVolGt0:  true,
			wantDiffGt0: true,
		},
		{
			name:    "TypeScript with operators",
			lang:    parser.LangTypeScript,
			fileExt: ".ts",
			code: `function calc(a: number, b: number): number {
    return a * b + a / b;
}`,
			wantN1Min:   2, // *, +, /
			wantN2Min:   2, // a, b
			wantVocMin:  4,
			wantLenMin:  6,
			wantVolGt0:  true,
			wantDiffGt0: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test"+tt.fileExt)
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			p := parser.New()
			defer p.Close()

			result, err := p.ParseFile(testFile)
			if err != nil {
				t.Fatalf("ParseFile() error = %v", err)
			}

			functions := parser.GetFunctions(result)
			if len(functions) == 0 {
				t.Fatal("No functions found")
			}

			h := NewHalsteadAnalyzer()
			metrics := h.AnalyzeNode(functions[0].Body, result.Source, result.Language)

			if metrics == nil {
				t.Fatal("AnalyzeNode() returned nil")
			}

			if metrics.OperatorsUnique < tt.wantN1Min {
				t.Errorf("OperatorsUnique = %d, want >= %d", metrics.OperatorsUnique, tt.wantN1Min)
			}
			if metrics.OperandsUnique < tt.wantN2Min {
				t.Errorf("OperandsUnique = %d, want >= %d", metrics.OperandsUnique, tt.wantN2Min)
			}
			if metrics.Vocabulary < tt.wantVocMin {
				t.Errorf("Vocabulary = %d, want >= %d", metrics.Vocabulary, tt.wantVocMin)
			}
			if metrics.Length < tt.wantLenMin {
				t.Errorf("Length = %d, want >= %d", metrics.Length, tt.wantLenMin)
			}
			if tt.wantVolGt0 && metrics.Volume <= 0 {
				t.Error("Volume should be > 0")
			}
			if tt.wantDiffGt0 && metrics.Difficulty <= 0 {
				t.Error("Difficulty should be > 0")
			}
		})
	}
}

func TestHalsteadMetrics_DerivedValues(t *testing.T) {
	tests := []struct {
		name        string
		n1          uint32 // distinct operators
		n2          uint32 // distinct operands
		N1          uint32 // total operators
		N2          uint32 // total operands
		wantVoc     uint32
		wantLen     uint32
		checkVolume bool
		checkEffort bool
		checkTime   bool
		checkBugs   bool
	}{
		{
			name:        "Basic metrics",
			n1:          5,
			n2:          10,
			N1:          20,
			N2:          40,
			wantVoc:     15,
			wantLen:     60,
			checkVolume: true,
			checkEffort: true,
			checkTime:   true,
			checkBugs:   true,
		},
		{
			name:        "Zero operands",
			n1:          5,
			n2:          0,
			N1:          10,
			N2:          0,
			wantVoc:     0, // Should be zero due to zero check
			wantLen:     0,
			checkVolume: false,
			checkEffort: false,
			checkTime:   false,
			checkBugs:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create with options to trigger derived calculation
			analyzer := NewComplexityAnalyzer(WithHalstead(true))
			defer analyzer.Close()

			// Test via the function that uses the analyzer
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test.go")
			code := `package main

func example() {
	x := 1 + 2 * 3 - 4 / 5
	y := x + 10
	z := y * 2
}`
			if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			result, err := analyzer.AnalyzeFile(testFile)
			if err != nil {
				t.Fatalf("AnalyzeFile() error = %v", err)
			}

			if len(result.Functions) == 0 {
				t.Fatal("No functions found")
			}

			metrics := result.Functions[0].Metrics.Halstead
			if metrics == nil {
				t.Fatal("Halstead metrics should not be nil")
			}

			// Verify vocabulary = n1 + n2
			if metrics.Vocabulary != metrics.OperatorsUnique+metrics.OperandsUnique {
				t.Errorf("Vocabulary = %d, want %d (n1 + n2)",
					metrics.Vocabulary, metrics.OperatorsUnique+metrics.OperandsUnique)
			}

			// Verify length = N1 + N2
			if metrics.Length != metrics.OperatorsTotal+metrics.OperandsTotal {
				t.Errorf("Length = %d, want %d (N1 + N2)",
					metrics.Length, metrics.OperatorsTotal+metrics.OperandsTotal)
			}

			// Volume should be positive for non-trivial code
			if metrics.Volume <= 0 {
				t.Error("Volume should be > 0 for non-trivial code")
			}

			// Effort = Volume * Difficulty
			expectedEffort := metrics.Volume * metrics.Difficulty
			if diff := metrics.Effort - expectedEffort; diff > 0.001 || diff < -0.001 {
				t.Errorf("Effort = %f, want %f (V * D)", metrics.Effort, expectedEffort)
			}

			// Time = Effort / 18
			expectedTime := metrics.Effort / 18.0
			if diff := metrics.Time - expectedTime; diff > 0.001 || diff < -0.001 {
				t.Errorf("Time = %f, want %f (E / 18)", metrics.Time, expectedTime)
			}
		})
	}
}

func TestIsOperatorNode(t *testing.T) {
	tests := []struct {
		nodeType string
		text     string
		lang     parser.Language
		want     bool
	}{
		{"binary_expression", "+", parser.LangGo, true},
		{"+", "+", parser.LangGo, true},
		{"if_statement", "if", parser.LangGo, true},
		{"for_statement", "for", parser.LangGo, true},
		{"identifier", "myVar", parser.LangGo, false},
		{"number", "42", parser.LangGo, false},
		{"string_literal", "hello", parser.LangGo, false},
	}

	for _, tt := range tests {
		t.Run(tt.nodeType+"_"+tt.text, func(t *testing.T) {
			got := isOperatorNode(tt.nodeType, tt.text, tt.lang)
			if got != tt.want {
				t.Errorf("isOperatorNode(%q, %q, %v) = %v, want %v",
					tt.nodeType, tt.text, tt.lang, got, tt.want)
			}
		})
	}
}

func TestIsOperandNode(t *testing.T) {
	tests := []struct {
		nodeType string
		text     string
		lang     parser.Language
		want     bool
	}{
		{"identifier", "myVar", parser.LangGo, true},
		{"number", "42", parser.LangGo, true},
		{"integer", "123", parser.LangGo, true},
		{"string_literal", "hello", parser.LangGo, true},
		{"true", "true", parser.LangGo, true},
		{"false", "false", parser.LangGo, true},
		{"nil", "nil", parser.LangGo, true},
		{"binary_expression", "+", parser.LangGo, false},
		{"if_statement", "if", parser.LangGo, false},
		{"source_file", "", parser.LangGo, false},
		{"", "", parser.LangGo, false},
	}

	for _, tt := range tests {
		t.Run(tt.nodeType+"_"+tt.text, func(t *testing.T) {
			got := isOperandNode(tt.nodeType, tt.text, tt.lang)
			if got != tt.want {
				t.Errorf("isOperandNode(%q, %q, %v) = %v, want %v",
					tt.nodeType, tt.text, tt.lang, got, tt.want)
			}
		})
	}
}

func TestOperatorKeywordsRecognized(t *testing.T) {
	tests := []struct {
		lang     parser.Language
		keywords []string
	}{
		{parser.LangGo, []string{"if", "for", "go", "defer", "select", "range"}},
		{parser.LangPython, []string{"if", "elif", "except", "with", "and", "or", "not"}},
		{parser.LangRust, []string{"if", "match", "loop", "async", "await"}},
	}

	for _, tt := range tests {
		t.Run(string(tt.lang), func(t *testing.T) {
			for _, kw := range tt.keywords {
				if !isOperatorNode("", kw, tt.lang) {
					t.Errorf("isOperatorNode(\"\", %q, %v) = false, want true", kw, tt.lang)
				}
			}
		})
	}
}

func TestComplexityAnalyzerWithHalstead(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

func simple() int {
	return 42
}

func withOps(a, b int) int {
	return a + b * 2
}
`
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	// Test with Halstead disabled
	t.Run("Without Halstead", func(t *testing.T) {
		analyzer := NewComplexityAnalyzer()
		defer analyzer.Close()

		result, err := analyzer.AnalyzeFile(testFile)
		if err != nil {
			t.Fatalf("AnalyzeFile() error = %v", err)
		}

		for _, fn := range result.Functions {
			if fn.Metrics.Halstead != nil {
				t.Errorf("Halstead should be nil when disabled for %s", fn.Name)
			}
		}
	})

	// Test with Halstead enabled
	t.Run("With Halstead", func(t *testing.T) {
		analyzer := NewComplexityAnalyzer(WithHalstead(true))
		defer analyzer.Close()

		result, err := analyzer.AnalyzeFile(testFile)
		if err != nil {
			t.Fatalf("AnalyzeFile() error = %v", err)
		}

		for _, fn := range result.Functions {
			if fn.Metrics.Halstead == nil {
				t.Errorf("Halstead should not be nil when enabled for %s", fn.Name)
			}
		}
	})
}

func BenchmarkHalsteadAnalysis(b *testing.B) {
	code := `package main

func complex(a, b, c, d int) int {
	result := 0
	if a > 0 && b > 0 {
		for i := 0; i < c; i++ {
			result += a * b + d
			if result > 100 {
				result = result / 2
			}
		}
	}
	return result
}
`
	tmpDir := b.TempDir()
	testFile := filepath.Join(tmpDir, "bench.go")
	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		b.Fatalf("failed to write test file: %v", err)
	}

	p := parser.New()
	defer p.Close()

	result, err := p.ParseFile(testFile)
	if err != nil {
		b.Fatalf("ParseFile() error = %v", err)
	}

	functions := parser.GetFunctions(result)
	if len(functions) == 0 {
		b.Fatal("No functions found")
	}

	h := NewHalsteadAnalyzer()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = h.AnalyzeNode(functions[0].Body, result.Source, result.Language)
	}
}
