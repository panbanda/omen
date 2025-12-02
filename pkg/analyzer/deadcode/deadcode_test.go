package deadcode

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestNew(t *testing.T) {
	a := New()
	if a == nil {
		t.Fatal("New() returned nil")
	}
	if a.parser == nil {
		t.Error("analyzer.parser is nil")
	}
	if a.confidence != 0.8 {
		t.Errorf("default confidence = %f, want 0.8", a.confidence)
	}
	if !a.buildGraph {
		t.Error("buildGraph should default to true")
	}
	a.Close()
}

func TestNewWithOptions(t *testing.T) {
	a := New(
		WithConfidence(0.9),
		WithSkipCallGraph(),
		WithMaxFileSize(1024),
	)

	if a.confidence != 0.9 {
		t.Errorf("confidence = %f, want 0.9", a.confidence)
	}
	if a.buildGraph {
		t.Error("WithSkipCallGraph should disable buildGraph")
	}
	if a.maxFileSize != 1024 {
		t.Errorf("maxFileSize = %d, want 1024", a.maxFileSize)
	}
	a.Close()
}

func TestAnalyzeFile_Go(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	code := `package main

func main() {
	used()
}

func used() int {
	return 42
}

func unused() int {
	return 0
}

var usedVar = 1
var unusedVar = 2
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	result, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if result.path != path {
		t.Errorf("path = %q, want %q", result.path, path)
	}

	// Should find function definitions
	if len(result.definitions) < 3 {
		t.Errorf("len(definitions) = %d, want >= 3", len(result.definitions))
	}

	// Should track usages
	if !result.usages["used"] {
		t.Error("'used' should be in usages")
	}
}

func TestAnalyzeFile_Python(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.py")

	code := `def main():
    used()

def used():
    return 42

def unused():
    return 0
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	result, err := a.AnalyzeFile(path)
	if err != nil {
		t.Fatalf("AnalyzeFile failed: %v", err)
	}

	if len(result.definitions) < 3 {
		t.Errorf("len(definitions) = %d, want >= 3", len(result.definitions))
	}
}

func TestAnalyzeProject(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "main.go")
	code1 := `package main

func main() {
	helper()
}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	file2 := filepath.Join(tmpDir, "helper.go")
	code2 := `package main

func helper() int {
	return 42
}

func unused() int {
	return 0
}
`
	if err := os.WriteFile(file2, []byte(code2), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithConfidence(0.5)) // Lower confidence to catch more
	defer a.Close()

	analysis, err := a.AnalyzeProject([]string{file1, file2})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis.Summary.TotalFilesAnalyzed != 2 {
		t.Errorf("TotalFilesAnalyzed = %d, want 2", analysis.Summary.TotalFilesAnalyzed)
	}
}

func TestIsTestFile(t *testing.T) {
	tests := []struct {
		path string
		want bool
	}{
		{"main_test.go", true},
		{"main.go", false},
		{"test_helper.py", false},
		{"helper_test.py", true},
		{"component.test.ts", true},
		{"component.spec.ts", true},
		{"component.ts", false},
		{"/src/test/helper.go", true},
		{"/src/__tests__/helper.js", true},
	}

	for _, tt := range tests {
		got := IsTestFile(tt.path)
		if got != tt.want {
			t.Errorf("IsTestFile(%q) = %v, want %v", tt.path, got, tt.want)
		}
	}
}

func TestHierarchicalBitSet(t *testing.T) {
	bs := NewHierarchicalBitSet(1000)

	// Test Set and IsSet
	bs.Set(42)
	if !bs.IsSet(42) {
		t.Error("IsSet(42) should be true after Set(42)")
	}
	if bs.IsSet(43) {
		t.Error("IsSet(43) should be false")
	}

	// Test CountSet
	if bs.CountSet() != 1 {
		t.Errorf("CountSet() = %d, want 1", bs.CountSet())
	}

	// Test SetBatch
	bs.SetBatch([]uint32{100, 200, 300})
	if bs.CountSet() != 4 {
		t.Errorf("CountSet() = %d, want 4", bs.CountSet())
	}
	if !bs.IsSet(100) || !bs.IsSet(200) || !bs.IsSet(300) {
		t.Error("SetBatch should set all provided indices")
	}
}

func TestVTableResolver(t *testing.T) {
	vt := NewVTableResolver()

	// Register a type with methods
	methods := map[string]uint32{
		"Foo": 1,
		"Bar": 2,
	}
	vt.RegisterType("MyType", "", methods)

	// Register implementation
	vt.RegisterImplementation("MyInterface", "MyType")

	// Resolve dynamic call
	targets := vt.ResolveDynamicCall("MyInterface", "Foo")
	if len(targets) != 1 {
		t.Fatalf("len(targets) = %d, want 1", len(targets))
	}
	if targets[0] != 1 {
		t.Errorf("targets[0] = %d, want 1", targets[0])
	}

	// Unknown method
	targets = vt.ResolveDynamicCall("MyInterface", "Unknown")
	if len(targets) != 0 {
		t.Errorf("len(targets) = %d, want 0 for unknown method", len(targets))
	}
}

func TestCoverageData(t *testing.T) {
	cd := NewCoverageData()

	// Test with no data
	if cd.IsLineCovered("test.go", 10) {
		t.Error("IsLineCovered should be false for empty coverage")
	}
	if cd.GetExecutionCount("test.go", 10) != 0 {
		t.Error("GetExecutionCount should be 0 for empty coverage")
	}

	// Add coverage data
	cd.CoveredLines["test.go"] = map[uint32]bool{10: true, 20: true}
	cd.ExecutionCounts["test.go"] = map[uint32]int64{10: 5, 20: 3}

	if !cd.IsLineCovered("test.go", 10) {
		t.Error("IsLineCovered should be true for covered line")
	}
	if cd.GetExecutionCount("test.go", 10) != 5 {
		t.Errorf("GetExecutionCount = %d, want 5", cd.GetExecutionCount("test.go", 10))
	}
}

func TestCrossLangReferenceGraph(t *testing.T) {
	g := NewCrossLangReferenceGraph()

	// Add nodes
	node1 := &ReferenceNode{ID: 1, Name: "func1"}
	node2 := &ReferenceNode{ID: 2, Name: "func2"}
	g.AddNode(node1)
	g.AddNode(node2)

	if len(g.Nodes) != 2 {
		t.Errorf("len(Nodes) = %d, want 2", len(g.Nodes))
	}

	// Add edge
	edge := ReferenceEdge{From: 1, To: 2, Type: RefDirectCall}
	g.AddEdge(edge)

	if len(g.Edges) != 1 {
		t.Errorf("len(Edges) = %d, want 1", len(g.Edges))
	}

	// Get outgoing edges
	outgoing := g.GetOutgoingEdges(1)
	if len(outgoing) != 1 {
		t.Errorf("len(outgoing) = %d, want 1", len(outgoing))
	}
	if outgoing[0].To != 2 {
		t.Errorf("outgoing[0].To = %d, want 2", outgoing[0].To)
	}
}

func TestGetVisibility(t *testing.T) {
	// Test Go visibility rules using parser.LangGo
	if got := getVisibility("PublicFunc", parser.LangGo); got != "public" {
		t.Errorf("getVisibility(PublicFunc, Go) = %q, want public", got)
	}
	if got := getVisibility("privateFunc", parser.LangGo); got != "private" {
		t.Errorf("getVisibility(privateFunc, Go) = %q, want private", got)
	}
}

func TestKind_String(t *testing.T) {
	tests := []struct {
		kind Kind
		want string
	}{
		{KindFunction, "unused_function"},
		{KindVariable, "unused_variable"},
		{KindClass, "unused_class"},
	}

	for _, tt := range tests {
		got := tt.kind.String()
		if got != tt.want {
			t.Errorf("Kind(%v).String() = %q, want %q", tt.kind, got, tt.want)
		}
	}
}

func TestReferenceType_String(t *testing.T) {
	tests := []struct {
		rt   ReferenceType
		want string
	}{
		{RefDirectCall, "direct_call"},
		{RefDynamicDispatch, "dynamic_dispatch"},
		{RefImport, "import"},
	}

	for _, tt := range tests {
		got := tt.rt.String()
		if got != tt.want {
			t.Errorf("ReferenceType(%v).String() = %q, want %q", tt.rt, got, tt.want)
		}
	}
}

func TestCallGraph_AddNode(t *testing.T) {
	g := NewCallGraph()

	node := &ReferenceNode{ID: 1, Name: "test", IsEntry: true}
	g.AddNode(node)

	if len(g.Nodes) != 1 {
		t.Errorf("len(Nodes) = %d, want 1", len(g.Nodes))
	}
	if len(g.EntryPoints) != 1 {
		t.Errorf("len(EntryPoints) = %d, want 1", len(g.EntryPoints))
	}
}

func TestSummary_Add(t *testing.T) {
	s := NewSummary()

	fn := Function{Name: "test", File: "test.go", Confidence: 0.9}
	fn.SetConfidenceLevel()
	s.AddFunction(fn)

	if s.TotalDeadFunctions != 1 {
		t.Errorf("TotalDeadFunctions = %d, want 1", s.TotalDeadFunctions)
	}

	v := Variable{Name: "var", File: "test.go", Confidence: 0.6}
	v.SetConfidenceLevel()
	s.AddVariable(v)

	if s.TotalDeadVariables != 1 {
		t.Errorf("TotalDeadVariables = %d, want 1", s.TotalDeadVariables)
	}

	c := Class{Name: "MyClass", File: "test.go", Confidence: 0.4}
	c.SetConfidenceLevel()
	s.AddClass(c)

	if s.TotalDeadClasses != 1 {
		t.Errorf("TotalDeadClasses = %d, want 1", s.TotalDeadClasses)
	}
}

func TestFunction_SetConfidenceLevel(t *testing.T) {
	tests := []struct {
		confidence float64
		want       ConfidenceLevel
	}{
		{0.9, ConfidenceHigh},
		{0.7, ConfidenceMedium},
		{0.4, ConfidenceLow},
	}

	for _, tt := range tests {
		f := Function{Confidence: tt.confidence}
		f.SetConfidenceLevel()
		if f.ConfidenceLevel != tt.want {
			t.Errorf("confidence %f -> level %q, want %q", tt.confidence, f.ConfidenceLevel, tt.want)
		}
	}
}
