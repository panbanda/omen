package duplicates

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/source"
)

func TestNew(t *testing.T) {
	a := New()
	if a == nil {
		t.Fatal("New() returned nil")
	}
	if a.parser == nil {
		t.Error("analyzer.parser is nil")
	}
	if a.config.MinTokens == 0 {
		t.Error("config should have default MinTokens")
	}
	a.Close()
}

func TestNewWithOptions(t *testing.T) {
	a := New(
		WithMinTokens(100),
		WithSimilarityThreshold(0.9),
		WithMaxFileSize(1024),
	)

	if a.config.MinTokens != 100 {
		t.Errorf("MinTokens = %d, want 100", a.config.MinTokens)
	}
	if a.config.SimilarityThreshold != 0.9 {
		t.Errorf("SimilarityThreshold = %f, want 0.9", a.config.SimilarityThreshold)
	}
	if a.maxFileSize != 1024 {
		t.Errorf("maxFileSize = %d, want 1024", a.maxFileSize)
	}
	a.Close()
}

func TestAnalyzeProject_ExactClones(t *testing.T) {
	tmpDir := t.TempDir()

	// Create two files with identical functions
	file1 := filepath.Join(tmpDir, "a.go")
	code1 := `package main

func duplicate() int {
	x := 1
	y := 2
	z := 3
	result := x + y + z
	if result > 5 {
		return result
	}
	return 0
}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	file2 := filepath.Join(tmpDir, "b.go")
	code2 := `package main

func duplicate() int {
	x := 1
	y := 2
	z := 3
	result := x + y + z
	if result > 5 {
		return result
	}
	return 0
}
`
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithMinTokens(10), WithSimilarityThreshold(0.8))
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{file1, file2}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if analysis.TotalFilesScanned != 2 {
		t.Errorf("TotalFilesScanned = %d, want 2", analysis.TotalFilesScanned)
	}

	// Should find at least one clone group
	if len(analysis.Groups) < 1 {
		t.Errorf("expected at least 1 clone group, got %d", len(analysis.Groups))
	}
}

func TestAnalyzeProject_NoClones(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "a.go")
	code1 := `package main

func funcA() int {
	return 1
}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	file2 := filepath.Join(tmpDir, "b.go")
	code2 := `package main

func funcB() string {
	return "hello"
}
`
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithMinTokens(50)) // High threshold to avoid small matches
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{file1, file2}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	if len(analysis.Clones) != 0 {
		t.Errorf("expected no clones, got %d", len(analysis.Clones))
	}
}

func TestAnalyzeProject_EmptyFiles(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "a.go")
	if err := os.WriteFile(file1, []byte("package main\n"), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{file1}, source.NewFilesystem())
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Empty/minimal files should not produce clones
	if len(analysis.Clones) != 0 {
		t.Errorf("expected no clones from minimal file, got %d", len(analysis.Clones))
	}
}

func TestMinHashSignature_JaccardSimilarity(t *testing.T) {
	sig1 := &MinHashSignature{Values: []uint64{1, 2, 3, 4, 5}}
	sig2 := &MinHashSignature{Values: []uint64{1, 2, 3, 4, 5}}

	sim := sig1.JaccardSimilarity(sig2)
	if sim != 1.0 {
		t.Errorf("identical signatures should have similarity 1.0, got %f", sim)
	}

	sig3 := &MinHashSignature{Values: []uint64{10, 20, 30, 40, 50}}
	sim = sig1.JaccardSimilarity(sig3)
	if sim != 0.0 {
		t.Errorf("completely different signatures should have similarity 0.0, got %f", sim)
	}
}

func TestTokenize(t *testing.T) {
	code := `func main() { x := 1 }`
	tokens := tokenize(code)

	// := is tokenized as : and = separately
	expected := []string{"func", "main", "(", ")", "{", "x", ":", "=", "1", "}"}
	if len(tokens) != len(expected) {
		t.Errorf("token count = %d, want %d", len(tokens), len(expected))
		t.Logf("got: %v", tokens)
		return
	}

	for i, tok := range expected {
		if tokens[i] != tok {
			t.Errorf("token[%d] = %q, want %q", i, tokens[i], tok)
		}
	}
}

func TestIsKeyword(t *testing.T) {
	keywords := []string{"func", "return", "if", "for", "def", "fn", "let"}
	for _, kw := range keywords {
		if !isKeyword(kw) {
			t.Errorf("expected %q to be a keyword", kw)
		}
	}

	nonKeywords := []string{"main", "foo", "bar", "x"}
	for _, nk := range nonKeywords {
		if isKeyword(nk) {
			t.Errorf("expected %q to not be a keyword", nk)
		}
	}
}

func TestIsLiteral(t *testing.T) {
	literals := []string{`"hello"`, "'x'", "`template`", "123", "-42", "3.14"}
	for _, lit := range literals {
		if !isLiteral(lit) {
			t.Errorf("expected %q to be a literal", lit)
		}
	}

	nonLiterals := []string{"foo", "func", "+", "("}
	for _, nl := range nonLiterals {
		if isLiteral(nl) {
			t.Errorf("expected %q to not be a literal", nl)
		}
	}
}

func TestDetermineCloneType(t *testing.T) {
	tests := []struct {
		similarity float64
		want       Type
	}{
		{1.0, Type1},
		{0.95, Type1},
		{0.90, Type2},
		{0.85, Type2},
		{0.80, Type3},
		{0.50, Type3},
	}

	for _, tt := range tests {
		got := determineCloneType(tt.similarity)
		if got != tt.want {
			t.Errorf("determineCloneType(%f) = %v, want %v", tt.similarity, got, tt.want)
		}
	}
}

func TestDetectLanguage(t *testing.T) {
	tests := []struct {
		path string
		want string
	}{
		{"test.go", "go"},
		{"test.rs", "rust"},
		{"test.py", "python"},
		{"test.ts", "typescript"},
		{"test.tsx", "typescript"},
		{"test.js", "javascript"},
		{"test.jsx", "javascript"},
		{"test.java", "java"},
		{"test.unknown", "unknown"},
	}

	for _, tt := range tests {
		got := detectLanguage(tt.path)
		if got != tt.want {
			t.Errorf("detectLanguage(%q) = %q, want %q", tt.path, got, tt.want)
		}
	}
}

func TestIsFunctionStart(t *testing.T) {
	tests := []struct {
		line string
		lang string
		want bool
	}{
		{"func main() {", "go", true},
		{"func (s *Server) Start()", "go", true},
		{"var x = 1", "go", false},
		{"fn main() {", "rust", true},
		{"let x = 1", "rust", false},
		{"def my_func():", "python", true},
		{"class MyClass:", "python", false},
		{"function hello() {", "javascript", true},
		{"const x = () => {", "javascript", true},
	}

	for _, tt := range tests {
		got := isFunctionStart(tt.line, tt.lang)
		if got != tt.want {
			t.Errorf("isFunctionStart(%q, %q) = %v, want %v", tt.line, tt.lang, got, tt.want)
		}
	}
}

func TestConfig_Defaults(t *testing.T) {
	cfg := DefaultConfig()

	if cfg.MinTokens <= 0 {
		t.Error("MinTokens should be positive")
	}
	if cfg.SimilarityThreshold <= 0 || cfg.SimilarityThreshold > 1 {
		t.Error("SimilarityThreshold should be in (0, 1]")
	}
	if cfg.NumHashFunctions <= 0 {
		t.Error("NumHashFunctions should be positive")
	}
	if cfg.NumBands <= 0 {
		t.Error("NumBands should be positive")
	}
}

func TestType_String(t *testing.T) {
	tests := []struct {
		typ  Type
		want string
	}{
		{Type1, "type1"},
		{Type2, "type2"},
		{Type3, "type3"},
	}

	for _, tt := range tests {
		got := tt.typ.String()
		if got != tt.want {
			t.Errorf("Type(%v).String() = %q, want %q", tt.typ, got, tt.want)
		}
	}
}

func TestSummary_AddClone(t *testing.T) {
	s := NewSummary()

	clone := Clone{
		Type:       Type1,
		Similarity: 0.95,
		FileA:      "a.go",
		FileB:      "b.go",
		LinesA:     10,
		LinesB:     10,
	}

	s.AddClone(clone)

	if s.TotalClones != 1 {
		t.Errorf("TotalClones = %d, want 1", s.TotalClones)
	}
	if s.Type1Count != 1 {
		t.Errorf("Type1Count = %d, want 1", s.Type1Count)
	}
	if s.DuplicatedLines != 20 {
		t.Errorf("DuplicatedLines = %d, want 20", s.DuplicatedLines)
	}
}
