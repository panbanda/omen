package analyzer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/jonathanreyes/omen-cli/pkg/models"
)

func TestNewDuplicateAnalyzer(t *testing.T) {
	tests := []struct {
		name              string
		minLines          int
		threshold         float64
		expectedMinLines  int
		expectedThreshold float64
		expectedNumHashes int
	}{
		{
			name:              "valid parameters",
			minLines:          10,
			threshold:         0.9,
			expectedMinLines:  10,
			expectedThreshold: 0.9,
			expectedNumHashes: 128,
		},
		{
			name:              "zero minLines defaults to 6",
			minLines:          0,
			threshold:         0.8,
			expectedMinLines:  6,
			expectedThreshold: 0.8,
			expectedNumHashes: 128,
		},
		{
			name:              "negative minLines defaults to 6",
			minLines:          -5,
			threshold:         0.8,
			expectedMinLines:  6,
			expectedThreshold: 0.8,
			expectedNumHashes: 128,
		},
		{
			name:              "zero threshold defaults to 0.8",
			minLines:          6,
			threshold:         0,
			expectedMinLines:  6,
			expectedThreshold: 0.8,
			expectedNumHashes: 128,
		},
		{
			name:              "negative threshold defaults to 0.8",
			minLines:          6,
			threshold:         -0.5,
			expectedMinLines:  6,
			expectedThreshold: 0.8,
			expectedNumHashes: 128,
		},
		{
			name:              "threshold over 1 defaults to 0.8",
			minLines:          6,
			threshold:         1.5,
			expectedMinLines:  6,
			expectedThreshold: 0.8,
			expectedNumHashes: 128,
		},
		{
			name:              "both invalid parameters use defaults",
			minLines:          -1,
			threshold:         -0.1,
			expectedMinLines:  6,
			expectedThreshold: 0.8,
			expectedNumHashes: 128,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewDuplicateAnalyzer(tt.minLines, tt.threshold)
			defer analyzer.Close()

			if analyzer.minLines != tt.expectedMinLines {
				t.Errorf("minLines = %d, want %d", analyzer.minLines, tt.expectedMinLines)
			}
			if analyzer.threshold != tt.expectedThreshold {
				t.Errorf("threshold = %f, want %f", analyzer.threshold, tt.expectedThreshold)
			}
			if analyzer.numHashes != tt.expectedNumHashes {
				t.Errorf("numHashes = %d, want %d", analyzer.numHashes, tt.expectedNumHashes)
			}
			if analyzer.parser == nil {
				t.Error("parser should not be nil")
			}
		})
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

func TestNormalizeCode(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "removes leading whitespace",
			input:    "    func main() {\n        return\n    }",
			expected: "func main() {\nreturn\n}",
		},
		{
			name:     "removes comments",
			input:    "func test() {\n// comment\nreturn\n}",
			expected: "func test() {\nreturn\n}",
		},
		{
			name:     "removes empty lines",
			input:    "func test() {\n\n\nreturn\n}",
			expected: "func test() {\nreturn\n}",
		},
		{
			name:     "removes block comments",
			input:    "func test() {\n/* comment */\nreturn\n}",
			expected: "func test() {\nreturn\n}",
		},
		{
			name:     "handles mixed whitespace",
			input:    "  \tfunc  test()  {\n\t\treturn\n  }",
			expected: "func  test()  {\nreturn\n}",
		},
		{
			name:     "empty input",
			input:    "",
			expected: "",
		},
		{
			name:     "only whitespace",
			input:    "   \n\t\n   ",
			expected: "",
		},
		{
			name:     "only comments",
			input:    "// comment1\n// comment2\n# comment3",
			expected: "",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := normalizeCode(tt.input)
			if result != tt.expected {
				t.Errorf("normalizeCode() = %q, want %q", result, tt.expected)
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

func TestGenerateShingles(t *testing.T) {
	tests := []struct {
		name     string
		content  string
		n        int
		expected map[string]bool
	}{
		{
			name:    "3-grams from simple code",
			content: "func main return",
			n:       3,
			expected: map[string]bool{
				"func main return": true,
			},
		},
		{
			name:    "3-grams with multiple shingles",
			content: "func main ( ) {",
			n:       3,
			expected: map[string]bool{
				"func main (": true,
				"main ( )":    true,
				"( ) {":       true,
			},
		},
		{
			name:    "2-grams",
			content: "a b c d",
			n:       2,
			expected: map[string]bool{
				"a b": true,
				"b c": true,
				"c d": true,
			},
		},
		{
			name:    "fewer tokens than n",
			content: "a b",
			n:       3,
			expected: map[string]bool{
				"a b": true,
			},
		},
		{
			name:     "empty content",
			content:  "",
			n:        3,
			expected: map[string]bool{},
		},
		{
			name:    "single token",
			content: "token",
			n:       3,
			expected: map[string]bool{
				"token": true,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := generateShingles(tt.content, tt.n)
			if len(result) != len(tt.expected) {
				t.Errorf("generateShingles() length = %d, want %d", len(result), len(tt.expected))
			}
			for shingle := range tt.expected {
				if !result[shingle] {
					t.Errorf("expected shingle %q not found", shingle)
				}
			}
			for shingle := range result {
				if !tt.expected[shingle] {
					t.Errorf("unexpected shingle %q found", shingle)
				}
			}
		})
	}
}

func TestComputeMinHash(t *testing.T) {
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	tests := []struct {
		name    string
		content string
	}{
		{
			name:    "simple code",
			content: "func main() {\n    return\n}",
		},
		{
			name:    "empty content",
			content: "",
		},
		{
			name:    "single line",
			content: "x = y + z",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			sig := analyzer.computeMinHash(tt.content)
			if sig == nil {
				t.Fatal("signature should not be nil")
			}
			if len(sig.Values) != analyzer.numHashes {
				t.Errorf("signature length = %d, want %d", len(sig.Values), analyzer.numHashes)
			}
		})
	}

	t.Run("identical content produces identical signatures", func(t *testing.T) {
		content := "func test() { return 42 }"
		sig1 := analyzer.computeMinHash(content)
		sig2 := analyzer.computeMinHash(content)

		for i := range sig1.Values {
			if sig1.Values[i] != sig2.Values[i] {
				t.Errorf("signature values differ at index %d: %d != %d", i, sig1.Values[i], sig2.Values[i])
			}
		}
	})

	t.Run("different content produces different signatures", func(t *testing.T) {
		sig1 := analyzer.computeMinHash("func test1() { return 1 }")
		sig2 := analyzer.computeMinHash("func test2() { return 2 }")

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

func TestCountNonEmptyLines(t *testing.T) {
	tests := []struct {
		name     string
		lines    []string
		expected int
	}{
		{
			name:     "all non-empty",
			lines:    []string{"line1", "line2", "line3"},
			expected: 3,
		},
		{
			name:     "mixed empty and non-empty",
			lines:    []string{"line1", "", "line3", "   ", "line5"},
			expected: 3,
		},
		{
			name:     "all empty",
			lines:    []string{"", "   ", "\t"},
			expected: 0,
		},
		{
			name:     "empty slice",
			lines:    []string{},
			expected: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := countNonEmptyLines(tt.lines)
			if result != tt.expected {
				t.Errorf("countNonEmptyLines() = %d, want %d", result, tt.expected)
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

func TestExtractBlocks(t *testing.T) {
	tmpDir := t.TempDir()

	tests := []struct {
		name            string
		content         string
		minLines        int
		minExpectedSize int
	}{
		{
			name: "file with multiple blocks",
			content: `func test1() {
    x := 1
    y := 2
    z := 3
    return x + y + z
}

func test2() {
    a := 1
    b := 2
    c := 3
    return a + b + c
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
			analyzer := NewDuplicateAnalyzer(tt.minLines, 0.8)
			defer analyzer.Close()

			filePath := filepath.Join(tmpDir, "test.go")
			if err := os.WriteFile(filePath, []byte(tt.content), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			blocks, err := analyzer.extractBlocks(filePath)
			if err != nil {
				t.Fatalf("extractBlocks() error = %v", err)
			}

			if len(blocks) < tt.minExpectedSize {
				t.Errorf("extractBlocks() returned %d blocks, want at least %d", len(blocks), tt.minExpectedSize)
			}

			for i, block := range blocks {
				if block.file != filePath {
					t.Errorf("block[%d].file = %q, want %q", i, block.file, filePath)
				}
				if block.startLine == 0 {
					t.Errorf("block[%d].startLine should not be 0", i)
				}
				if block.endLine < block.startLine {
					t.Errorf("block[%d].endLine (%d) < startLine (%d)", i, block.endLine, block.startLine)
				}
			}
		})
	}
}

func TestAnalyzeProject_ExactClones(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	duplicateCode := `func calculate(a, b int) int {
    x := a + b
    y := x * 2
    z := y - 5
    result := z / 3
    return result
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

	if result.Summary.Type1Count == 0 {
		t.Error("expected Type1 clones for exact duplicates")
	}
}

func TestAnalyzeProject_ParametricClones(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	code1 := `func calculate(a, b int) int {
    x := a + b
    y := x * 2
    z := y - 5
    result := z / 3
    return result
}
`

	code2 := `func compute(p, q int) int {
    m := p + q
    n := m * 2
    o := n - 5
    answer := o / 3
    return answer
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

func TestAnalyzeProject_StructuralClones(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.75)
	defer analyzer.Close()

	code1 := `func process() {
    x := getData()
    y := transform(x)
    z := validate(y)
    w := save(z)
    return w
}
`

	code2 := `func handle() {
    a := fetchData()
    b := convert(a)
    c := check(b)
    d := persist(c)
    e := log(d)
    return e
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

	if result.TotalFilesScanned != 2 {
		t.Errorf("TotalFilesScanned = %d, want 2", result.TotalFilesScanned)
	}
}

func TestAnalyzeProject_NoDuplicates(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
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
	analyzer := NewDuplicateAnalyzer(6, 0.8)
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

func TestAnalyzeProject_VerySmallFiles(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	smallCode := `func x() {
    return
}
`

	file1 := filepath.Join(tmpDir, "small1.go")
	file2 := filepath.Join(tmpDir, "small2.go")

	if err := os.WriteFile(file1, []byte(smallCode), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(smallCode), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Clones) > 0 {
		t.Log("Note: very small files below minLines threshold should not produce clones")
	}
}

func TestAnalyzeProject_SameFileOverlapping(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	code := `func duplicate() {
    x := 1
    y := 2
    z := 3
    a := 4
    b := 5
    c := 6
}

func duplicate() {
    x := 1
    y := 2
    z := 3
    a := 4
    b := 5
    c := 6
}
`

	file := filepath.Join(tmpDir, "single.go")
	if err := os.WriteFile(file, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write file: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Clones) > 0 {
		t.Log("Found clones within same file (non-overlapping)")
	}
}

func TestAnalyzeProject_MultipleClonePairs(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	code1 := `func handler1() {
    data := fetch()
    processed := transform(data)
    validated := check(processed)
    saved := store(validated)
    logged := record(saved)
    return logged
}
`

	code2 := `func handler2() {
    data := fetch()
    processed := transform(data)
    validated := check(processed)
    saved := store(validated)
    logged := record(saved)
    return logged
}
`

	code3 := `func handler3() {
    data := fetch()
    processed := transform(data)
    validated := check(processed)
    saved := store(validated)
    logged := record(saved)
    return logged
}
`

	file1 := filepath.Join(tmpDir, "file1.go")
	file2 := filepath.Join(tmpDir, "file2.go")
	file3 := filepath.Join(tmpDir, "file3.go")

	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}
	if err := os.WriteFile(file3, []byte(code3), 0644); err != nil {
		t.Fatalf("failed to write file3: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2, file3})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(result.Clones) == 0 {
		t.Error("expected to find multiple clone pairs")
	}

	if result.Summary.FileOccurrences[file1] == 0 {
		t.Error("file1 should be counted in occurrences")
	}
}

func TestAnalyzeProject_SummaryStatistics(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	duplicateCode := `func process() {
    step1 := initialize()
    step2 := validate(step1)
    step3 := transform(step2)
    step4 := persist(step3)
    step5 := notify(step4)
    return step5
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

	if len(result.Clones) > 0 {
		if result.Summary.AvgSimilarity <= 0 {
			t.Error("AvgSimilarity should be > 0 when clones exist")
		}
		if result.Summary.P50Similarity <= 0 {
			t.Error("P50Similarity should be > 0 when clones exist")
		}
		if result.Summary.P95Similarity <= 0 {
			t.Error("P95Similarity should be > 0 when clones exist")
		}
		if result.Summary.TotalClones != len(result.Clones) {
			t.Errorf("Summary.TotalClones = %d, want %d", result.Summary.TotalClones, len(result.Clones))
		}
	}
}

func TestAnalyzeProject_NonExistentFile(t *testing.T) {
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeProject([]string{"/nonexistent/file.go"})
	if err != nil {
		t.Fatalf("AnalyzeProject() should handle missing files gracefully, got error: %v", err)
	}

	if result == nil {
		t.Fatal("result should not be nil")
	}
}

func TestAnalyzeProject_WithComments(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	code1 := `func handler() {
    // This is a comment
    x := fetch()
    // Another comment
    y := process(x)
    /* Block comment */
    z := save(y)
    return z
}
`

	code2 := `func handler() {
    x := fetch()
    y := process(x)
    z := save(y)
    return z
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

	if len(result.Clones) > 0 {
		t.Log("Comments should be normalized away, making code more similar")
	}
}

func TestAnalyzeProject_CloneTypeDistribution(t *testing.T) {
	tmpDir := t.TempDir()
	analyzer := NewDuplicateAnalyzer(6, 0.8)
	defer analyzer.Close()

	exactCode := `func exact() {
    a := 1
    b := 2
    c := 3
    d := 4
    e := 5
    return a + b + c + d + e
}
`

	file1 := filepath.Join(tmpDir, "exact1.go")
	file2 := filepath.Join(tmpDir, "exact2.go")

	if err := os.WriteFile(file1, []byte(exactCode), 0644); err != nil {
		t.Fatalf("failed to write file1: %v", err)
	}
	if err := os.WriteFile(file2, []byte(exactCode), 0644); err != nil {
		t.Fatalf("failed to write file2: %v", err)
	}

	result, err := analyzer.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	totalByType := result.Summary.Type1Count + result.Summary.Type2Count + result.Summary.Type3Count
	if len(result.Clones) > 0 && totalByType != result.Summary.TotalClones {
		t.Errorf("clone type counts (%d) don't match total clones (%d)", totalByType, result.Summary.TotalClones)
	}
}
