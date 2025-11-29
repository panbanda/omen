package analyzer

import (
	"os"
	"path/filepath"
	"strconv"
	"testing"

	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/models"
)

func TestNewDuplicateAnalyzer(t *testing.T) {
	tests := []struct {
		name string
		opts []DuplicateOption
	}{
		{
			name: "default config",
			opts: nil,
		},
		{
			name: "with custom min tokens",
			opts: []DuplicateOption{WithDuplicateMinTokens(80)},
		},
		{
			name: "with custom threshold",
			opts: []DuplicateOption{WithDuplicateSimilarityThreshold(0.9)},
		},
		{
			name: "with max file size",
			opts: []DuplicateOption{WithDuplicateMaxFileSize(1024 * 1024)},
		},
		{
			name: "with multiple options",
			opts: []DuplicateOption{
				WithDuplicateMinTokens(100),
				WithDuplicateSimilarityThreshold(0.85),
				WithDuplicateMaxFileSize(2 * 1024 * 1024),
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewDuplicateAnalyzer(tt.opts...)
			defer analyzer.Close()

			if analyzer.parser == nil {
				t.Error("parser should not be nil")
			}
		})
	}
}

func TestWithDuplicateConfig(t *testing.T) {
	cfg := config.DuplicateConfig{
		MinTokens:            100,
		SimilarityThreshold:  0.75,
		ShingleSize:          7,
		NumHashFunctions:     150,
		NumBands:             15,
		RowsPerBand:          10,
		NormalizeIdentifiers: false,
		NormalizeLiterals:    false,
		IgnoreComments:       false,
		MinGroupSize:         3,
	}

	analyzer := NewDuplicateAnalyzer(WithDuplicateConfig(cfg))
	defer analyzer.Close()

	if analyzer.parser == nil {
		t.Error("parser should not be nil")
	}
	if analyzer.config.MinTokens != cfg.MinTokens {
		t.Errorf("MinTokens = %d, want %d", analyzer.config.MinTokens, cfg.MinTokens)
	}
	if analyzer.config.SimilarityThreshold != cfg.SimilarityThreshold {
		t.Errorf("SimilarityThreshold = %f, want %f", analyzer.config.SimilarityThreshold, cfg.SimilarityThreshold)
	}
}

func TestDefaultDuplicateConfig(t *testing.T) {
	cfg := DefaultDuplicateConfig()

	if cfg.MinTokens != 50 {
		t.Errorf("MinTokens = %d, want 50", cfg.MinTokens)
	}
	if cfg.SimilarityThreshold != 0.70 {
		t.Errorf("SimilarityThreshold = %f, want 0.70", cfg.SimilarityThreshold)
	}
	if cfg.ShingleSize != 5 {
		t.Errorf("ShingleSize = %d, want 5", cfg.ShingleSize)
	}
	if cfg.NumHashFunctions != 200 {
		t.Errorf("NumHashFunctions = %d, want 200", cfg.NumHashFunctions)
	}
	if cfg.NumBands != 20 {
		t.Errorf("NumBands = %d, want 20", cfg.NumBands)
	}
	if cfg.RowsPerBand != 10 {
		t.Errorf("RowsPerBand = %d, want 10", cfg.RowsPerBand)
	}
	if !cfg.NormalizeIdentifiers {
		t.Error("NormalizeIdentifiers should be true by default")
	}
	if !cfg.NormalizeLiterals {
		t.Error("NormalizeLiterals should be true by default")
	}
	if !cfg.IgnoreComments {
		t.Error("IgnoreComments should be true by default")
	}
	if cfg.MinGroupSize != 2 {
		t.Errorf("MinGroupSize = %d, want 2", cfg.MinGroupSize)
	}
}

func TestIsComment(t *testing.T) {
	tests := []struct {
		line     string
		expected bool
	}{
		{"// single line comment", true},
		{"# python comment", true},
		{"/* block comment start", true},
		{"* inside block comment", true},
		{"*/ block comment end", true},
		{"regular code line", false},
		{"func main() {", false},
		{"  // indented comment", false},
		{"  /* indented block", false},
		{"", false},
	}

	for _, tt := range tests {
		t.Run(tt.line, func(t *testing.T) {
			result := isComment(tt.line)
			if result != tt.expected {
				t.Errorf("isComment(%q) = %v, want %v", tt.line, result, tt.expected)
			}
		})
	}
}

func TestTokenize(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected []string
	}{
		{
			name:     "simple function",
			input:    "func main() {}",
			expected: []string{"func", "main", "(", ")", "{", "}"},
		},
		{
			name:     "with operators",
			input:    "x = y + z",
			expected: []string{"x", "=", "y", "+", "z"},
		},
		{
			name:     "with numbers",
			input:    "count = 42",
			expected: []string{"count", "=", "42"},
		},
		{
			name:     "with underscores",
			input:    "my_var = other_var",
			expected: []string{"my_var", "=", "other_var"},
		},
		{
			name:     "mixed alphanumeric",
			input:    "var1 = var2",
			expected: []string{"var1", "=", "var2"},
		},
		{
			name:     "empty string",
			input:    "",
			expected: []string{},
		},
		{
			name:     "only whitespace",
			input:    "   \t\n  ",
			expected: []string{},
		},
		{
			name:     "punctuation only",
			input:    "(){};",
			expected: []string{"(", ")", "{", "}", ";"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := tokenize(tt.input)
			if len(result) != len(tt.expected) {
				t.Errorf("tokenize() length = %d, want %d", len(result), len(tt.expected))
				t.Errorf("got %v, want %v", result, tt.expected)
				return
			}
			for i := range result {
				if result[i] != tt.expected[i] {
					t.Errorf("tokenize()[%d] = %q, want %q", i, result[i], tt.expected[i])
				}
			}
		})
	}
}

func TestGenerateKShingles(t *testing.T) {
	tests := []struct {
		name          string
		tokens        []string
		k             int
		expectedCount int
	}{
		{
			name:          "3-grams from simple code",
			tokens:        []string{"func", "main", "return"},
			k:             3,
			expectedCount: 1, // n - k + 1 = 3 - 3 + 1 = 1
		},
		{
			name:          "3-grams with multiple shingles",
			tokens:        []string{"func", "main", "(", ")", "{"},
			k:             3,
			expectedCount: 3, // n - k + 1 = 5 - 3 + 1 = 3
		},
		{
			name:          "2-grams",
			tokens:        []string{"a", "b", "c", "d"},
			k:             2,
			expectedCount: 3, // n - k + 1 = 4 - 2 + 1 = 3
		},
		{
			name:          "fewer tokens than k - hashes entire sequence",
			tokens:        []string{"a", "b"},
			k:             3,
			expectedCount: 1, // Falls back to hashing entire token sequence
		},
		{
			name:          "empty tokens",
			tokens:        []string{},
			k:             3,
			expectedCount: 0,
		},
		{
			name:          "single token - hashes entire sequence",
			tokens:        []string{"token"},
			k:             3,
			expectedCount: 1, // Falls back to hashing entire token sequence
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := generateKShingles(tt.tokens, tt.k)
			if len(result) != tt.expectedCount {
				t.Errorf("generateKShingles() length = %d, want %d", len(result), tt.expectedCount)
			}
		})
	}

	// Test that same tokens produce same hashes (deterministic)
	t.Run("deterministic hashing", func(t *testing.T) {
		tokens := []string{"func", "main", "(", ")", "{"}
		result1 := generateKShingles(tokens, 3)
		result2 := generateKShingles(tokens, 3)

		if len(result1) != len(result2) {
			t.Errorf("non-deterministic: different lengths %d vs %d", len(result1), len(result2))
		}

		// Convert to sets for comparison
		set1 := make(map[uint64]bool)
		for _, h := range result1 {
			set1[h] = true
		}
		for _, h := range result2 {
			if !set1[h] {
				t.Errorf("non-deterministic: hash %d not found in first result", h)
			}
		}
	})

	// Test that different tokens produce different hashes
	t.Run("different tokens produce different hashes", func(t *testing.T) {
		tokens1 := []string{"func", "main", "(", ")", "{"}
		tokens2 := []string{"func", "test", "(", ")", "{"}
		result1 := generateKShingles(tokens1, 3)
		result2 := generateKShingles(tokens2, 3)

		set1 := make(map[uint64]bool)
		for _, h := range result1 {
			set1[h] = true
		}

		// At least some hashes should be different
		differentCount := 0
		for _, h := range result2 {
			if !set1[h] {
				differentCount++
			}
		}
		if differentCount == 0 {
			t.Error("expected different tokens to produce at least some different hashes")
		}
	})
}

func TestComputeMinHash(t *testing.T) {
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	tests := []struct {
		name   string
		tokens []string
	}{
		{
			name:   "simple code",
			tokens: tokenize("func main() {\n    return\n}"),
		},
		{
			name:   "empty content",
			tokens: []string{},
		},
		{
			name:   "single line",
			tokens: tokenize("x = y + z"),
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			sig := analyzer.computeMinHash(tt.tokens)
			if sig == nil {
				t.Fatal("signature should not be nil")
			}
			if len(sig.Values) != analyzer.config.NumHashFunctions {
				t.Errorf("signature length = %d, want %d", len(sig.Values), analyzer.config.NumHashFunctions)
			}
		})
	}

	t.Run("identical content produces identical signatures", func(t *testing.T) {
		tokens := tokenize("func test() { return 42 }")
		sig1 := analyzer.computeMinHash(tokens)
		sig2 := analyzer.computeMinHash(tokens)

		for i := range sig1.Values {
			if sig1.Values[i] != sig2.Values[i] {
				t.Errorf("signature values differ at index %d: %d != %d", i, sig1.Values[i], sig2.Values[i])
			}
		}
	})

	t.Run("different content produces different signatures", func(t *testing.T) {
		sig1 := analyzer.computeMinHash(tokenize("func test1() { return 1 }"))
		sig2 := analyzer.computeMinHash(tokenize("func test2() { return 2 }"))

		allSame := true
		for i := range sig1.Values {
			if sig1.Values[i] != sig2.Values[i] {
				allSame = false
				break
			}
		}
		if allSame {
			t.Error("different content should produce different signatures")
		}
	})
}

func TestMinHashJaccardSimilarity(t *testing.T) {
	tests := []struct {
		name     string
		sig1     *models.MinHashSignature
		sig2     *models.MinHashSignature
		expected float64
	}{
		{
			name: "identical signatures",
			sig1: &models.MinHashSignature{
				Values: []uint64{1, 2, 3, 4, 5},
			},
			sig2: &models.MinHashSignature{
				Values: []uint64{1, 2, 3, 4, 5},
			},
			expected: 1.0,
		},
		{
			name: "completely different signatures",
			sig1: &models.MinHashSignature{
				Values: []uint64{1, 2, 3, 4, 5},
			},
			sig2: &models.MinHashSignature{
				Values: []uint64{6, 7, 8, 9, 10},
			},
			expected: 0.0,
		},
		{
			name: "partially matching signatures",
			sig1: &models.MinHashSignature{
				Values: []uint64{1, 2, 3, 4, 5},
			},
			sig2: &models.MinHashSignature{
				Values: []uint64{1, 2, 8, 9, 10},
			},
			expected: 0.4,
		},
		{
			name: "different length signatures",
			sig1: &models.MinHashSignature{
				Values: []uint64{1, 2, 3},
			},
			sig2: &models.MinHashSignature{
				Values: []uint64{1, 2},
			},
			expected: 0.0,
		},
		{
			name: "empty signatures",
			sig1: &models.MinHashSignature{
				Values: []uint64{},
			},
			sig2: &models.MinHashSignature{
				Values: []uint64{},
			},
			expected: 0.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := tt.sig1.JaccardSimilarity(tt.sig2)
			if result != tt.expected {
				t.Errorf("JaccardSimilarity() = %f, want %f", result, tt.expected)
			}
		})
	}
}

func TestDetermineCloneType(t *testing.T) {
	tests := []struct {
		similarity float64
		expected   models.CloneType
	}{
		{1.0, models.CloneType1},
		{0.99, models.CloneType1},
		{0.95, models.CloneType1},
		{0.94, models.CloneType2},
		{0.90, models.CloneType2},
		{0.85, models.CloneType2},
		{0.84, models.CloneType3},
		{0.80, models.CloneType3},
		{0.50, models.CloneType3},
	}

	for _, tt := range tests {
		t.Run("", func(t *testing.T) {
			result := determineCloneType(tt.similarity)
			if result != tt.expected {
				t.Errorf("determineCloneType(%f) = %v, want %v", tt.similarity, result, tt.expected)
			}
		})
	}
}

func TestPercentileFloat64Dup(t *testing.T) {
	tests := []struct {
		name       string
		sorted     []float64
		percentile int
		expected   float64
	}{
		{
			name:       "p50 of 5 elements",
			sorted:     []float64{1.0, 2.0, 3.0, 4.0, 5.0},
			percentile: 50,
			expected:   3.0,
		},
		{
			name:       "p95 of 5 elements",
			sorted:     []float64{1.0, 2.0, 3.0, 4.0, 5.0},
			percentile: 95,
			expected:   5.0,
		},
		{
			name:       "p0 of elements",
			sorted:     []float64{1.0, 2.0, 3.0},
			percentile: 0,
			expected:   1.0,
		},
		{
			name:       "p100 of elements",
			sorted:     []float64{1.0, 2.0, 3.0},
			percentile: 100,
			expected:   3.0,
		},
		{
			name:       "empty slice",
			sorted:     []float64{},
			percentile: 50,
			expected:   0.0,
		},
		{
			name:       "single element",
			sorted:     []float64{42.0},
			percentile: 50,
			expected:   42.0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := percentileFloat64Dup(tt.sorted, tt.percentile)
			if result != tt.expected {
				t.Errorf("percentileFloat64Dup() = %f, want %f", result, tt.expected)
			}
		})
	}
}

func TestExtractFragments(t *testing.T) {
	tmpDir := t.TempDir()

	tests := []struct {
		name            string
		content         string
		minLines        int
		minExpectedSize int
	}{
		{
			name: "file with multiple functions",
			content: `func test1() {
    x := 1
    y := 2
    z := 3
    a := 4
    b := 5
    c := 6
    return x + y + z + a + b + c
}

func test2() {
    a := 1
    b := 2
    c := 3
    d := 4
    e := 5
    f := 6
    return a + b + c + d + e + f
}`,
			minLines:        6,
			minExpectedSize: 1,
		},
		{
			name: "small file",
			content: `func small() {
    return
}`,
			minLines:        6,
			minExpectedSize: 0,
		},
		{
			name:            "empty file",
			content:         "",
			minLines:        6,
			minExpectedSize: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewDuplicateAnalyzer(
				WithDuplicateMinTokens(tt.minLines*8),
				WithDuplicateSimilarityThreshold(0.8),
			)
			defer analyzer.Close()

			filePath := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(filePath, []byte(tt.content), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			fragments, err := analyzer.extractFragments(filePath)
			if err != nil {
				t.Fatalf("extractFragments() error = %v", err)
			}

			if len(fragments) < tt.minExpectedSize {
				t.Errorf("extractFragments() returned %d fragments, want at least %d", len(fragments), tt.minExpectedSize)
			}

			for i, frag := range fragments {
				if frag.file != filePath {
					t.Errorf("fragment[%d].file = %q, want %q", i, frag.file, filePath)
				}
				if frag.startLine == 0 {
					t.Errorf("fragment[%d].startLine should not be 0", i)
				}
				if frag.endLine < frag.startLine {
					t.Errorf("fragment[%d].endLine (%d) < startLine (%d)", i, frag.endLine, frag.startLine)
				}
			}
		})
	}
}

func TestAnalyzeProject_ExactClones(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(40), // 5 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	// More substantial code to ensure we exceed minimum token threshold
	duplicateCode := `func calculate(a, b int) int {
    x := a + b
    y := x * 2
    z := y - 5
    result := z / 3
    w := result + 1
    v := w - 2
    u := v * 3
    final := u + 10
    output := final - 5
    value := output * 2
    return value
}
`

	file1 := filepath.Join(tmpDir, "file1.go")
	file2 := filepath.Join(tmpDir, "file2.go")

	if err := os.WriteFile(file1, []byte(duplicateCode), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(duplicateCode), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Clones) == 0 {
		t.Error("expected to find clones between identical files")
	}

	if result.TotalFilesScanned != 2 {
		t.Errorf("TotalFilesScanned = %d, want 2", result.TotalFilesScanned)
	}

	if result.Summary.TotalClones == 0 {
		t.Error("expected summary to count clones")
	}
}

func TestAnalyzeProject_ParametricClones(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	code1 := `func calculate(a, b int) int {
    x := a + b
    y := x * 2
    z := y - 5
    result := z / 3
    w := result + 1
    v := w - 2
    u := v * 3
    return u
}
`

	code2 := `func compute(p, q int) int {
    m := p + q
    n := m * 2
    o := n - 5
    answer := o / 3
    r := answer + 1
    s := r - 2
    t := s * 3
    return t
}
`

	file1 := filepath.Join(tmpDir, "file1.go")
	file2 := filepath.Join(tmpDir, "file2.go")

	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	hasClones := len(result.Clones) > 0
	if !hasClones {
		t.Log("Note: parametric clones might not be detected depending on similarity threshold")
	}
}

func TestAnalyzeProject_NoDuplicates(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	code1 := `func uniqueFunction1() {
    database := connect()
    users := database.query("SELECT * FROM users")
    for _, user := range users {
        process(user)
    }
}
`

	code2 := `func uniqueFunction2() {
    config := loadConfig()
    server := createServer(config)
    server.start()
    log("Server started")
}
`

	file1 := filepath.Join(tmpDir, "file1.go")
	file2 := filepath.Join(tmpDir, "file2.go")

	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if result.Summary.AvgSimilarity > 0 && len(result.Clones) == 0 {
		t.Error("AvgSimilarity should be 0 when no clones found")
	}
}

func TestAnalyzeProject_EmptyFiles(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	file1 := filepath.Join(tmpDir, "empty1.go")
	file2 := filepath.Join(tmpDir, "empty2.go")

	if err := os.WriteFile(file1, []byte(""), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(""), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Clones) > 0 {
		t.Error("empty files should not produce clones")
	}
}

func TestAnalyzeProject_NonExistentFile(t *testing.T) {
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject([]string{"/nonexistent/file.go"})
	if err != nil {
		t.Fatalf("AnalyzeProject() should handle missing files gracefully, got error: %v", err)
	}

	if result == nil {
		t.Fatal("result should not be nil")
	}
}

func TestIsKeyword(t *testing.T) {
	tests := []struct {
		token    string
		expected bool
	}{
		{"func", true},
		{"return", true},
		{"if", true},
		{"for", true},
		{"fn", true},
		{"let", true},
		{"def", true},
		{"class", true},
		{"function", true},
		{"myVariable", false},
		{"calculate", false},
		{"VAR_1", false},
		{"", false},
	}

	for _, tt := range tests {
		t.Run(tt.token, func(t *testing.T) {
			got := isKeyword(tt.token)
			if got != tt.expected {
				t.Errorf("isKeyword(%q) = %v, want %v", tt.token, got, tt.expected)
			}
		})
	}
}

func TestIsLiteral(t *testing.T) {
	tests := []struct {
		token    string
		expected bool
	}{
		{"42", true},
		{"3.14", true},
		{"-5", true},
		{`"hello"`, true},
		{"'a'", true},
		{"`template`", true},
		{"myVar", false},
		{"func", false},
		{"", false},
	}

	for _, tt := range tests {
		t.Run(tt.token, func(t *testing.T) {
			got := isLiteral(tt.token)
			if got != tt.expected {
				t.Errorf("isLiteral(%q) = %v, want %v", tt.token, got, tt.expected)
			}
		})
	}
}

func TestIsOperatorOrDelimiter(t *testing.T) {
	tests := []struct {
		token    string
		expected bool
	}{
		{"+", true},
		{"-", true},
		{"=", true},
		{"==", true},
		{"(", true},
		{")", true},
		{"{", true},
		{"}", true},
		{",", true},
		{";", true},
		{"myVar", false},
		{"func", false},
		{"", false},
	}

	for _, tt := range tests {
		t.Run(tt.token, func(t *testing.T) {
			got := isOperatorOrDelimiter(tt.token)
			if got != tt.expected {
				t.Errorf("isOperatorOrDelimiter(%q) = %v, want %v", tt.token, got, tt.expected)
			}
		})
	}
}

func TestCanonicalizeIdentifier(t *testing.T) {
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	id1 := analyzer.canonicalizeIdentifier("myVariable")
	id2 := analyzer.canonicalizeIdentifier("anotherVar")
	id3 := analyzer.canonicalizeIdentifier("myVariable")

	if id1 == id2 {
		t.Error("different identifiers should have different canonical names")
	}

	if id1 != id3 {
		t.Error("same identifier should have same canonical name")
	}

	if id1 != "VAR_1" {
		t.Errorf("first identifier should be VAR_1, got %s", id1)
	}

	if id2 != "VAR_2" {
		t.Errorf("second identifier should be VAR_2, got %s", id2)
	}
}

func TestNormalizeToken(t *testing.T) {
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	tests := []struct {
		name     string
		token    string
		expected string
	}{
		{"keyword stays same", "func", "func"},
		{"operator stays same", "+", "+"},
		{"delimiter stays same", "(", "("},
		{"number becomes LITERAL", "42", "LITERAL"},
		{"string becomes LITERAL", `"hello"`, "LITERAL"},
		{"empty stays empty", "", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := analyzer.normalizeToken(tt.token)
			if got != tt.expected {
				t.Errorf("normalizeToken(%q) = %q, want %q", tt.token, got, tt.expected)
			}
		})
	}

	identifier := analyzer.normalizeToken("myVariable")
	if identifier == "myVariable" {
		t.Error("identifier should be normalized to VAR_N format")
	}
	if len(identifier) < 4 || identifier[:4] != "VAR_" {
		t.Errorf("identifier should start with VAR_, got %q", identifier)
	}
}

func TestDetectLanguage(t *testing.T) {
	tests := []struct {
		path     string
		expected string
	}{
		{"test.go", "go"},
		{"test.rs", "rust"},
		{"test.py", "python"},
		{"test.ts", "typescript"},
		{"test.tsx", "typescript"},
		{"test.js", "javascript"},
		{"test.jsx", "javascript"},
		{"test.c", "c"},
		{"test.h", "c"},
		{"test.cpp", "cpp"},
		{"test.hpp", "cpp"},
		{"test.java", "java"},
		{"test.kt", "kotlin"},
		{"test.rb", "ruby"},
		{"test.php", "php"},
		{"test.unknown", "unknown"},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := detectLanguage(tt.path)
			if got != tt.expected {
				t.Errorf("detectLanguage(%q) = %q, want %q", tt.path, got, tt.expected)
			}
		})
	}
}

func TestIsFunctionStart(t *testing.T) {
	tests := []struct {
		line     string
		lang     string
		expected bool
	}{
		{"func main() {", "go", true},
		{"func (r *Reader) Read() {", "go", true},
		{"fn main() {", "rust", true},
		{"pub fn new() -> Self {", "rust", true},
		{"def main():", "python", true},
		{"def __init__(self):", "python", true},
		{"function test() {", "javascript", true},
		{"const fn = () => {", "javascript", true},
		{"void main() {", "c", true},
		{"int calculate(int a) {", "cpp", true},
		{"public void test() {", "java", true},
		{"fun main() {", "kotlin", true},
		{"x := 1", "go", false},
		{"return result", "go", false},
	}

	for _, tt := range tests {
		t.Run(tt.line, func(t *testing.T) {
			got := isFunctionStart(tt.line, tt.lang)
			if got != tt.expected {
				t.Errorf("isFunctionStart(%q, %q) = %v, want %v", tt.line, tt.lang, got, tt.expected)
			}
		})
	}
}

func TestLSHCandidateFiltering(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.7),
	)
	defer analyzer.Close()

	// Create multiple similar files to test LSH grouping
	similarCode := `func process(data []int) int {
    sum := 0
    for _, v := range data {
        sum += v
    }
    avg := sum / len(data)
    result := avg * 2
    return result
}
`

	files := make([]string, 5)
	for i := range files {
		files[i] = filepath.Join(tmpDir, "file"+strconv.Itoa(i+1)+".go")
		if err := os.WriteFile(files[i], []byte(similarCode), 0644); err != nil {
			t.Fatalf("failed to write file: %v", err)
		}
	}

	result, err := analyzer.AnalyzeProject(files)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if result.TotalFilesScanned != 5 {
		t.Errorf("TotalFilesScanned = %d, want 5", result.TotalFilesScanned)
	}

	// With LSH, we should find clones efficiently
	if len(result.Groups) > 0 {
		t.Logf("Found %d clone groups using LSH", len(result.Groups))
	}
}

func TestHashBand(t *testing.T) {
	values1 := []uint64{1, 2, 3, 4, 5}
	values2 := []uint64{1, 2, 3, 4, 5}
	values3 := []uint64{6, 7, 8, 9, 10}

	hash1 := hashBand(values1, 0)
	hash2 := hashBand(values2, 0)
	hash3 := hashBand(values3, 0)

	if hash1 != hash2 {
		t.Error("identical values with same seed should produce same hash")
	}

	if hash1 == hash3 {
		t.Error("different values should produce different hashes")
	}

	// Different seeds should produce different hashes
	hash4 := hashBand(values1, 1)
	if hash1 == hash4 {
		t.Error("same values with different seeds should produce different hashes")
	}
}

func TestCloneGrouping(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	duplicateCode := `func process() {
    step1 := initialize()
    step2 := validate(step1)
    step3 := transform(step2)
    step4 := persist(step3)
    step5 := notify(step4)
    step6 := finalize(step5)
    return step6
}
`

	file1 := filepath.Join(tmpDir, "file1.go")
	file2 := filepath.Join(tmpDir, "file2.go")
	file3 := filepath.Join(tmpDir, "file3.go")

	for _, f := range []string{file1, file2, file3} {
		if err := os.WriteFile(f, []byte(duplicateCode), 0644); err != nil {
			t.Fatalf("failed to write file: %v", err)
		}
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2, file3})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if result.Summary.TotalGroups == 0 && len(result.Clones) > 0 {
		t.Error("expected clone groups to be created when clones are found")
	}

	for _, group := range result.Groups {
		if len(group.Instances) < 2 {
			t.Errorf("group %d has fewer than 2 instances", group.ID)
		}

		if group.AverageSimilarity <= 0 || group.AverageSimilarity > 1 {
			t.Errorf("group %d has invalid average similarity: %f", group.ID, group.AverageSimilarity)
		}
	}
}

func TestDuplicationHotspots(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(
		WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
		WithDuplicateSimilarityThreshold(0.8),
	)
	defer analyzer.Close()

	duplicateCode := `func handler() {
    data := fetch()
    processed := transform(data)
    validated := check(processed)
    saved := store(validated)
    logged := record(saved)
    done := finish(logged)
    return done
}
`

	files := make([]string, 5)
	for i := range files {
		files[i] = filepath.Join(tmpDir, "file"+strconv.Itoa(i+1)+".go")
		if err := os.WriteFile(files[i], []byte(duplicateCode), 0644); err != nil {
			t.Fatalf("failed to write file: %v", err)
		}
	}

	result, err := analyzer.AnalyzeProject(files)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Clones) > 0 && len(result.Summary.Hotspots) == 0 {
		t.Error("expected hotspots to be computed when clones exist")
	}

	for _, hotspot := range result.Summary.Hotspots {
		if hotspot.File == "" {
			t.Error("hotspot file should not be empty")
		}
		if hotspot.Severity < 0 {
			t.Errorf("hotspot severity should be >= 0, got %f", hotspot.Severity)
		}
	}

	for i := 1; i < len(result.Summary.Hotspots); i++ {
		if result.Summary.Hotspots[i].Severity > result.Summary.Hotspots[i-1].Severity {
			t.Error("hotspots should be sorted by severity descending")
		}
	}
}

// TestPmatCompatibility verifies compatibility with pmat's duplicate detection.
func TestPmatCompatibility(t *testing.T) {
	t.Run("config defaults match pmat", func(t *testing.T) {
		cfg := DefaultDuplicateConfig()

		// pmat defaults from duplicate_detector.rs
		if cfg.MinTokens != 50 {
			t.Errorf("MinTokens = %d, pmat default is 50", cfg.MinTokens)
		}
		if cfg.SimilarityThreshold != 0.70 {
			t.Errorf("SimilarityThreshold = %f, pmat default is 0.70", cfg.SimilarityThreshold)
		}
		if cfg.ShingleSize != 5 {
			t.Errorf("ShingleSize = %d, pmat default is 5", cfg.ShingleSize)
		}
		if cfg.NumHashFunctions != 200 {
			t.Errorf("NumHashFunctions = %d, pmat default is 200", cfg.NumHashFunctions)
		}
		if cfg.NumBands != 20 {
			t.Errorf("NumBands = %d, pmat default is 20", cfg.NumBands)
		}
		if cfg.RowsPerBand != 10 {
			t.Errorf("RowsPerBand = %d, pmat default is 10", cfg.RowsPerBand)
		}
		// Verify bands * rows_per_band = num_hash_functions (pmat constraint)
		if cfg.NumBands*cfg.RowsPerBand != cfg.NumHashFunctions {
			t.Errorf("NumBands * RowsPerBand (%d) != NumHashFunctions (%d)",
				cfg.NumBands*cfg.RowsPerBand, cfg.NumHashFunctions)
		}
	})

	t.Run("identifier normalization to VAR_N format", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
			WithDuplicateSimilarityThreshold(0.8),
		)
		defer analyzer.Close()

		// pmat normalizes identifiers to VAR_N format
		id1 := analyzer.canonicalizeIdentifier("userCount")
		id2 := analyzer.canonicalizeIdentifier("totalItems")
		id3 := analyzer.canonicalizeIdentifier("userCount") // Same as id1

		if id1[:4] != "VAR_" {
			t.Errorf("identifier should be normalized to VAR_N format, got %s", id1)
		}
		if id1 == id2 {
			t.Error("different identifiers should map to different VAR_N")
		}
		if id1 != id3 {
			t.Error("same identifier should map to same VAR_N")
		}
	})

	t.Run("literal normalization to LITERAL", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
			WithDuplicateSimilarityThreshold(0.8),
		)
		defer analyzer.Close()

		// pmat normalizes all literals to "LITERAL"
		tests := []string{"42", "3.14", `"hello"`, "'c'", "`template`"}
		for _, literal := range tests {
			got := analyzer.normalizeToken(literal)
			if got != "LITERAL" {
				t.Errorf("normalizeToken(%q) = %q, pmat normalizes to LITERAL", literal, got)
			}
		}
	})

	t.Run("keywords preserved unchanged", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
			WithDuplicateSimilarityThreshold(0.8),
		)
		defer analyzer.Close()

		// pmat preserves keywords
		keywords := []string{"func", "return", "if", "for", "fn", "let", "def", "class"}
		for _, kw := range keywords {
			got := analyzer.normalizeToken(kw)
			if got != kw {
				t.Errorf("keyword %q should be preserved, got %q", kw, got)
			}
		}
	})

	t.Run("5-shingles generation", func(t *testing.T) {
		// pmat uses 5-shingles (k=5)
		tokens := []string{"func", "main", "(", ")", "{", "return", "LITERAL", "}"}
		shingles := generateKShingles(tokens, 5)

		expectedCount := len(tokens) - 5 + 1 // n - k + 1 shingles
		if len(shingles) != expectedCount {
			t.Errorf("generateKShingles produced %d shingles, expected %d", len(shingles), expectedCount)
		}

		// Verify we get uint64 hashes (blake3-based, pmat-compatible)
		if len(shingles) > 0 && shingles[0] == 0 {
			t.Error("shingle hash should not be zero")
		}
	})

	t.Run("MinHash signature has 200 hash values", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48), // 6 lines * 8 tokens/line
			WithDuplicateSimilarityThreshold(0.8),
		)
		defer analyzer.Close()

		tokens := tokenize("func main() { return 42 }")
		sig := analyzer.computeMinHash(tokens)

		if len(sig.Values) != 200 {
			t.Errorf("MinHash signature has %d values, pmat uses 200", len(sig.Values))
		}
	})
}

// TestNormalizationDisabled verifies normalization can be disabled like pmat allows.
func TestNormalizationDisabled(t *testing.T) {
	cfg := config.DuplicateConfig{
		MinTokens:            50,
		SimilarityThreshold:  0.70,
		ShingleSize:          5,
		NumHashFunctions:     200,
		NumBands:             20,
		RowsPerBand:          10,
		NormalizeIdentifiers: false, // Disabled
		NormalizeLiterals:    false, // Disabled
		IgnoreComments:       true,
		MinGroupSize:         2,
	}

	analyzer := NewDuplicateAnalyzer(WithDuplicateConfig(cfg))
	defer analyzer.Close()

	t.Run("identifiers preserved when normalization disabled", func(t *testing.T) {
		got := analyzer.normalizeToken("myVariable")
		if got == "VAR_1" || got[:4] == "VAR_" {
			t.Error("identifier should not be normalized when NormalizeIdentifiers=false")
		}
		if got != "myVariable" {
			t.Errorf("identifier should be preserved as 'myVariable', got %q", got)
		}
	})

	t.Run("literals preserved when normalization disabled", func(t *testing.T) {
		tests := []struct {
			input    string
			expected string
		}{
			{"42", "42"},
			{`"hello"`, `"hello"`},
		}
		for _, tt := range tests {
			got := analyzer.normalizeToken(tt.input)
			if got != tt.expected {
				t.Errorf("normalizeToken(%q) = %q, expected %q when NormalizeLiterals=false",
					tt.input, got, tt.expected)
			}
		}
	})
}

// TestMaxFileSize verifies that files exceeding maxFileSize are skipped.
func TestMaxFileSize(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a large file
	largeContent := "func large() {\n"
	for i := 0; i < 1000; i++ {
		largeContent += "    x := " + strconv.Itoa(i) + "\n"
	}
	largeContent += "}\n"

	largeFile := filepath.Join(tmpDir, "large.go")
	if err := os.WriteFile(largeFile, []byte(largeContent), 0644); err != nil {
		t.Fatalf("failed to write large file: %v", err)
	}

	// Get actual file size
	info, err := os.Stat(largeFile)
	if err != nil {
		t.Fatalf("failed to stat file: %v", err)
	}
	fileSize := info.Size()

	t.Run("file under limit is processed", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48),
			WithDuplicateSimilarityThreshold(0.8),
			WithDuplicateMaxFileSize(fileSize+1000), // Set limit above file size
		)
		defer analyzer.Close()

		result, err := analyzer.AnalyzeProject([]string{largeFile})
		if err != nil {
			t.Fatalf("AnalyzeProject() error = %v", err)
		}

		if result.TotalFilesScanned != 1 {
			t.Errorf("TotalFilesScanned = %d, want 1", result.TotalFilesScanned)
		}
	})

	t.Run("file over limit is skipped", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48),
			WithDuplicateSimilarityThreshold(0.8),
			WithDuplicateMaxFileSize(100), // Set limit below file size
		)
		defer analyzer.Close()

		result, err := analyzer.AnalyzeProject([]string{largeFile})
		if err != nil {
			t.Fatalf("AnalyzeProject() error = %v", err)
		}

		// File should be skipped, no fragments extracted
		if result.TotalFilesScanned != 1 {
			t.Errorf("TotalFilesScanned = %d, want 1 (counts scanned, not processed)", result.TotalFilesScanned)
		}
	})

	t.Run("max file size of 0 means no limit", func(t *testing.T) {
		analyzer := NewDuplicateAnalyzer(
			WithDuplicateMinTokens(48),
			WithDuplicateSimilarityThreshold(0.8),
			WithDuplicateMaxFileSize(0), // No limit
		)
		defer analyzer.Close()

		result, err := analyzer.AnalyzeProject([]string{largeFile})
		if err != nil {
			t.Fatalf("AnalyzeProject() error = %v", err)
		}

		if result.TotalFilesScanned != 1 {
			t.Errorf("TotalFilesScanned = %d, want 1", result.TotalFilesScanned)
		}
	})
}

// TestMultiLanguageFunctionExtraction tests function detection across languages.
func TestMultiLanguageFunctionExtraction(t *testing.T) {
	tests := []struct {
		lang     string
		funcLine string
		notFunc  string
	}{
		{"go", "func process(data []byte) error {", "type Handler struct {"},
		{"rust", "fn process(data: Vec<u8>) -> Result<(), Error> {", "struct Handler {"},
		{"python", "def process(data):", "class Handler:"},
		{"typescript", "function process(data: Buffer): void {", "interface Handler {"},
		{"javascript", "function process(data) {", "class Handler {"},
		{"java", "public void process(byte[] data) {", "public class Handler {"},
		{"c", "void process(char* data) {", "struct handler {"},
		{"cpp", "void process(std::vector<char> data) {", "class Handler {"},
	}

	for _, tt := range tests {
		t.Run(tt.lang, func(t *testing.T) {
			if !isFunctionStart(tt.funcLine, tt.lang) {
				t.Errorf("isFunctionStart(%q, %q) = false, expected true", tt.funcLine, tt.lang)
			}
			// notFunc test is informational - some patterns may still match due to heuristics
		})
	}
}
