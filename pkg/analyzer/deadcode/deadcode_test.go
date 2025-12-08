package deadcode

import (
	"context"
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

	analysis, err := a.Analyze(context.Background(), []string{file1, file2})
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

func TestWithCoverage(t *testing.T) {
	coverage := NewCoverageData()
	coverage.CoveredLines["test.go"] = map[uint32]bool{10: true}

	a := New(WithCoverage(coverage))
	defer a.Close()

	if a.coverageData == nil {
		t.Error("WithCoverage should set coverageData")
	}
	if !a.coverageData.IsLineCovered("test.go", 10) {
		t.Error("coverageData should contain test.go:10")
	}
}

func TestWithCapacity(t *testing.T) {
	a := New(WithCapacity(500_000))
	defer a.Close()

	if a.reachability.totalNodes != 500_000 {
		t.Errorf("capacity = %d, want 500000", a.reachability.totalNodes)
	}
}

func TestAnalyzeProject_WithSkipCallGraph(t *testing.T) {
	tmpDir := t.TempDir()

	file1 := filepath.Join(tmpDir, "main.go")
	code1 := `package main

func main() {
	helper()
}

func helper() {}

func unused() {}
`
	if err := os.WriteFile(file1, []byte(code1), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithSkipCallGraph(), WithConfidence(0.5))
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{file1})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Should still detect dead code using simple usage analysis
	if analysis.Summary.TotalFilesAnalyzed != 1 {
		t.Errorf("TotalFilesAnalyzed = %d, want 1", analysis.Summary.TotalFilesAnalyzed)
	}
}

func TestAnalyzeFile_TypeScript(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.ts")

	code := `class MyClass implements MyInterface {
	foo(): void {}
}

interface MyInterface {
	foo(): void;
}

function unused(): number {
	return 0;
}
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

	if len(result.definitions) < 2 {
		t.Errorf("len(definitions) = %d, want >= 2", len(result.definitions))
	}
}

func TestAnalyzeFile_Rust(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.rs")

	code := `fn main() {
    used();
}

fn used() -> i32 {
    42
}

#[no_mangle]
pub extern "C" fn ffi_function() -> i32 {
    0
}

fn unused() -> i32 {
    0
}
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

func TestAnalyzeFile_CWithFFI(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.c")

	code := `__declspec(dllexport) int exported_func() {
    return 42;
}

int internal_func() {
    return 0;
}
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

	if result == nil {
		t.Fatal("Expected non-nil result")
	}
}

func TestAnalyzeFile_JavaWithImports(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "Test.java")

	code := `package com.example;

import java.util.List;

public class Test {
    public void main() {
        helper();
    }

    private void helper() {}

    private void unused() {}
}
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

	if len(result.definitions) < 2 {
		t.Errorf("len(definitions) = %d, want >= 2", len(result.definitions))
	}
}

func TestCallGraph_AddEdge(t *testing.T) {
	g := NewCallGraph()

	node1 := &ReferenceNode{ID: 1, Name: "func1"}
	node2 := &ReferenceNode{ID: 2, Name: "func2"}
	g.AddNode(node1)
	g.AddNode(node2)

	edge := ReferenceEdge{From: 1, To: 2, Type: RefDirectCall, Confidence: 0.95}
	g.AddEdge(edge)

	if len(g.Edges) != 1 {
		t.Errorf("len(Edges) = %d, want 1", len(g.Edges))
	}

	outgoing := g.GetOutgoingEdges(1)
	if len(outgoing) != 1 {
		t.Errorf("len(outgoing) = %d, want 1", len(outgoing))
	}
	if outgoing[0].To != 2 {
		t.Errorf("outgoing[0].To = %d, want 2", outgoing[0].To)
	}

	// Test empty outgoing edges
	outgoing = g.GetOutgoingEdges(999)
	if len(outgoing) != 0 {
		t.Errorf("len(outgoing) = %d, want 0 for non-existent node", len(outgoing))
	}
}

func TestConfidenceLevel_String(t *testing.T) {
	tests := []struct {
		level ConfidenceLevel
		want  string
	}{
		{ConfidenceHigh, "High"},
		{ConfidenceMedium, "Medium"},
		{ConfidenceLow, "Low"},
	}

	for _, tt := range tests {
		got := tt.level.String()
		if got != tt.want {
			t.Errorf("ConfidenceLevel(%q).String() = %q, want %q", tt.level, got, tt.want)
		}
	}
}

func TestItemType_String(t *testing.T) {
	tests := []struct {
		itemType ItemType
		want     string
	}{
		{ItemTypeFunction, "function"},
		{ItemTypeClass, "class"},
		{ItemTypeVariable, "variable"},
		{ItemTypeUnreachable, "unreachable"},
	}

	for _, tt := range tests {
		got := tt.itemType.String()
		if got != tt.want {
			t.Errorf("ItemType(%q).String() = %q, want %q", tt.itemType, got, tt.want)
		}
	}
}

func TestSetConfidenceThresholds(t *testing.T) {
	original := GetConfidenceThresholds()
	defer SetConfidenceThresholds(original)

	SetConfidenceThresholds(ConfidenceThresholds{
		HighThreshold:   0.9,
		MediumThreshold: 0.6,
	})

	thresholds := GetConfidenceThresholds()
	if thresholds.HighThreshold != 0.9 {
		t.Errorf("HighThreshold = %f, want 0.9", thresholds.HighThreshold)
	}
	if thresholds.MediumThreshold != 0.6 {
		t.Errorf("MediumThreshold = %f, want 0.6", thresholds.MediumThreshold)
	}

	// Invalid thresholds should be ignored
	SetConfidenceThresholds(ConfidenceThresholds{
		HighThreshold:   0,
		MediumThreshold: 1.5,
	})
	thresholds = GetConfidenceThresholds()
	if thresholds.HighThreshold != 0.9 {
		t.Error("Invalid HighThreshold (0) should be ignored")
	}
	if thresholds.MediumThreshold != 0.6 {
		t.Error("Invalid MediumThreshold (1.5) should be ignored")
	}
}

func TestFileMetrics_CalculateScore(t *testing.T) {
	tests := []struct {
		name       string
		metrics    FileMetrics
		wantScore  float32
		checkRange bool
	}{
		{
			name: "high confidence",
			metrics: FileMetrics{
				DeadPercentage: 50.0,
				DeadLines:      100,
				DeadFunctions:  5,
				Confidence:     ConfidenceHigh,
			},
			checkRange: true,
		},
		{
			name: "medium confidence",
			metrics: FileMetrics{
				DeadPercentage: 50.0,
				DeadLines:      100,
				DeadFunctions:  5,
				Confidence:     ConfidenceMedium,
			},
			checkRange: true,
		},
		{
			name: "low confidence",
			metrics: FileMetrics{
				DeadPercentage: 50.0,
				DeadLines:      100,
				DeadFunctions:  5,
				Confidence:     ConfidenceLow,
			},
			checkRange: true,
		},
		{
			name: "capped dead lines",
			metrics: FileMetrics{
				DeadPercentage: 10.0,
				DeadLines:      2000, // Should be capped at 1000
				DeadFunctions:  100,  // Should be capped at 50
				Confidence:     ConfidenceHigh,
			},
			checkRange: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tt.metrics.CalculateScore()
			if tt.checkRange && (tt.metrics.DeadScore < 0 || tt.metrics.DeadScore > 200) {
				t.Errorf("DeadScore = %f, expected in range [0, 200]", tt.metrics.DeadScore)
			}
		})
	}
}

func TestFileMetrics_UpdatePercentage(t *testing.T) {
	fm := &FileMetrics{
		DeadLines:  50,
		TotalLines: 100,
	}

	fm.UpdatePercentage()

	if fm.DeadPercentage != 50.0 {
		t.Errorf("DeadPercentage = %f, want 50.0", fm.DeadPercentage)
	}

	// Test with zero total lines
	fm2 := &FileMetrics{
		DeadLines:  10,
		TotalLines: 0,
	}
	fm2.UpdatePercentage()
	if fm2.DeadPercentage != 0 {
		t.Errorf("DeadPercentage with 0 TotalLines = %f, want 0", fm2.DeadPercentage)
	}
}

func TestFileMetrics_AddItem(t *testing.T) {
	fm := &FileMetrics{Items: make([]Item, 0)}

	fm.AddItem(Item{Type: ItemTypeFunction, Name: "func1", Line: 10})
	if fm.DeadFunctions != 1 {
		t.Errorf("DeadFunctions = %d, want 1", fm.DeadFunctions)
	}
	if fm.DeadLines != 10 {
		t.Errorf("DeadLines = %d, want 10 (function estimate)", fm.DeadLines)
	}

	fm.AddItem(Item{Type: ItemTypeClass, Name: "Class1", Line: 20})
	if fm.DeadClasses != 1 {
		t.Errorf("DeadClasses = %d, want 1", fm.DeadClasses)
	}

	fm.AddItem(Item{Type: ItemTypeVariable, Name: "var1", Line: 30})
	if fm.DeadModules != 1 {
		t.Errorf("DeadModules = %d, want 1", fm.DeadModules)
	}

	fm.AddItem(Item{Type: ItemTypeUnreachable, Name: "block", Line: 40})
	if fm.UnreachableBlocks != 1 {
		t.Errorf("UnreachableBlocks = %d, want 1", fm.UnreachableBlocks)
	}

	if len(fm.Items) != 4 {
		t.Errorf("len(Items) = %d, want 4", len(fm.Items))
	}
}

func TestSummary_AddUnreachableBlock(t *testing.T) {
	s := NewSummary()

	block := UnreachableBlock{
		File:      "test.go",
		StartLine: 10,
		EndLine:   15,
		Reason:    "after return",
	}
	s.AddUnreachableBlock(block)

	if s.TotalUnreachableLines != 6 {
		t.Errorf("TotalUnreachableLines = %d, want 6", s.TotalUnreachableLines)
	}
	if s.ByFile["test.go"] != 6 {
		t.Errorf("ByFile[test.go] = %d, want 6", s.ByFile["test.go"])
	}
	if s.ByKind[KindUnreachable] != 6 {
		t.Errorf("ByKind[KindUnreachable] = %d, want 6", s.ByKind[KindUnreachable])
	}
}

func TestSummary_CalculatePercentage(t *testing.T) {
	s := NewSummary()
	s.TotalLinesAnalyzed = 1000
	s.TotalDeadFunctions = 5
	s.TotalDeadClasses = 2
	s.TotalDeadVariables = 10
	s.TotalUnreachableLines = 20

	s.CalculatePercentage()

	// Expected: 20 + (5*10) + (2*5) + 10 = 20 + 50 + 10 + 10 = 90 lines
	// 90 / 1000 * 100 = 9%
	expectedPercent := float64(90) / float64(1000) * 100
	if s.DeadCodePercentage != expectedPercent {
		t.Errorf("DeadCodePercentage = %f, want %f", s.DeadCodePercentage, expectedPercent)
	}

	// Test with graph data
	s2 := NewSummary()
	s2.TotalNodesInGraph = 100
	s2.ReachableNodes = 80
	s2.CalculatePercentage()

	if s2.UnreachableNodes != 20 {
		t.Errorf("UnreachableNodes = %d, want 20", s2.UnreachableNodes)
	}
}

func TestReport_NewReport(t *testing.T) {
	r := NewReport()

	if r.Files == nil {
		t.Error("Files should be initialized")
	}
	if r.Config.MinDeadLines != 10 {
		t.Errorf("MinDeadLines = %d, want 10", r.Config.MinDeadLines)
	}
}

func TestReport_FromAnalysis(t *testing.T) {
	analysis := &Analysis{
		DeadFunctions: []Function{
			{Name: "unused1", File: "test.go", Line: 10, Confidence: 0.95, Reason: "Not referenced"},
			{Name: "unused2", File: "test.go", Line: 20, Confidence: 0.65, Reason: "Not referenced"},
		},
		DeadClasses: []Class{
			{Name: "UnusedClass", File: "test.go", Line: 30, Reason: "Not instantiated"},
		},
		DeadVariables: []Variable{
			{Name: "unusedVar", File: "other.go", Line: 5, Reason: "Not used"},
		},
		UnreachableCode: []UnreachableBlock{
			{File: "test.go", StartLine: 50, EndLine: 52, Reason: "after return"},
		},
		Summary: Summary{
			TotalFilesAnalyzed:     2,
			TotalDeadFunctions:     2,
			TotalDeadClasses:       1,
			TotalDeadVariables:     1,
			TotalUnreachableBlocks: 1,
		},
	}

	r := NewReport()
	r.FromAnalysis(analysis)

	if r.TotalFiles != 2 {
		t.Errorf("TotalFiles = %d, want 2", r.TotalFiles)
	}
	if len(r.Files) != 2 {
		t.Errorf("len(Files) = %d, want 2 (test.go and other.go)", len(r.Files))
	}
	if r.Summary.DeadFunctions != 2 {
		t.Errorf("Summary.DeadFunctions = %d, want 2", r.Summary.DeadFunctions)
	}
}

func TestGetVisibility_AllLanguages(t *testing.T) {
	tests := []struct {
		name string
		lang parser.Language
		want string
	}{
		// Go
		{"PublicFunc", parser.LangGo, "public"},
		{"privateFunc", parser.LangGo, "private"},

		// Python
		{"public_func", parser.LangPython, "public"},
		{"_internal_func", parser.LangPython, "internal"},
		{"__private_func", parser.LangPython, "private"},

		// Ruby
		{"public_method", parser.LangRuby, "public"},
		{"_private", parser.LangRuby, "private"},

		// Unknown language
		{"anyFunc", parser.LangC, "unknown"},
	}

	for _, tt := range tests {
		got := getVisibility(tt.name, tt.lang)
		if got != tt.want {
			t.Errorf("getVisibility(%q, %v) = %q, want %q", tt.name, tt.lang, got, tt.want)
		}
	}
}

func TestClass_SetConfidenceLevel(t *testing.T) {
	tests := []struct {
		confidence float64
		want       ConfidenceLevel
	}{
		{0.9, ConfidenceHigh},
		{0.7, ConfidenceMedium},
		{0.4, ConfidenceLow},
	}

	for _, tt := range tests {
		c := Class{Confidence: tt.confidence}
		c.SetConfidenceLevel()
		if c.ConfidenceLevel != tt.want {
			t.Errorf("confidence %f -> level %q, want %q", tt.confidence, c.ConfidenceLevel, tt.want)
		}
	}
}

func TestVariable_SetConfidenceLevel(t *testing.T) {
	tests := []struct {
		confidence float64
		want       ConfidenceLevel
	}{
		{0.9, ConfidenceHigh},
		{0.7, ConfidenceMedium},
		{0.4, ConfidenceLow},
	}

	for _, tt := range tests {
		v := Variable{Confidence: tt.confidence}
		v.SetConfidenceLevel()
		if v.ConfidenceLevel != tt.want {
			t.Errorf("confidence %f -> level %q, want %q", tt.confidence, v.ConfidenceLevel, tt.want)
		}
	}
}

func TestAnalyzeFile_UnreachableCode(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test.go")

	code := `package main

func main() {
	return
	x := 1  // unreachable
	_ = x
}

func withPanic() {
	panic("error")
	y := 2  // unreachable
	_ = y
}
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

	if len(result.unreachableBlocks) < 1 {
		t.Errorf("Expected at least 1 unreachable block, got %d", len(result.unreachableBlocks))
	}
}

func TestAnalyzeProject_EmptyFiles(t *testing.T) {
	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if analysis == nil {
		t.Fatal("Expected non-nil analysis")
	}
	if analysis.Summary.TotalFilesAnalyzed != 0 {
		t.Errorf("TotalFilesAnalyzed = %d, want 0", analysis.Summary.TotalFilesAnalyzed)
	}
}

// Ruby-specific tests for call detection

func TestRuby_ImplicitMethodCallsAreTracked(t *testing.T) {
	// In Ruby, methods can call other methods without explicit receiver.
	// These should be tracked as call edges so the called methods aren't
	// incorrectly reported as dead code.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "order.rb")

	code := `class Order
  def process
    calculate_total
    validate_items
  end

  def calculate_total
    42
  end

  def validate_items
    true
  end

  def unused_method
    nil
  end
end
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

	// Should have 5 definitions: 1 class + 4 methods
	if len(result.definitions) != 5 {
		t.Errorf("len(definitions) = %d, want 5", len(result.definitions))
	}

	// Should track calls to calculate_total and validate_items
	hasCalculateTotal := false
	hasValidateItems := false
	for _, call := range result.calls {
		if call.callee == "calculate_total" {
			hasCalculateTotal = true
		}
		if call.callee == "validate_items" {
			hasValidateItems = true
		}
	}

	if !hasCalculateTotal {
		t.Error("expected call to 'calculate_total' to be tracked")
	}
	if !hasValidateItems {
		t.Error("expected call to 'validate_items' to be tracked")
	}
}

func TestRuby_ExplicitMethodCallsWithReceiverAreTracked(t *testing.T) {
	// Ruby method calls with explicit receiver like obj.method() or self.method()
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "service.rb")

	code := `class OrderService
  def process(order)
    order.validate
    self.log_action
  end

  def log_action
    puts "logged"
  end
end
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

	// Should track the call to log_action (via self.log_action)
	hasLogAction := false
	for _, call := range result.calls {
		if call.callee == "log_action" {
			hasLogAction = true
		}
	}

	if !hasLogAction {
		t.Error("expected call to 'log_action' (via self.log_action) to be tracked")
	}
}

func TestRuby_DefinitionNamesNotCountedAsUsages(t *testing.T) {
	// The name of a method/function definition should NOT count as a usage.
	// Otherwise every defined symbol would appear "used" by its own definition.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "model.rb")

	code := `class User
  def save
    # does nothing
  end
end
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

	// 'save' should be in definitions but NOT in usages
	if _, exists := result.definitions["save"]; !exists {
		t.Error("'save' should be in definitions")
	}
	if result.usages["save"] {
		t.Error("'save' should NOT be in usages (it's only defined, not called)")
	}
}

func TestRuby_RailsControllerActionsAreEntryPoints(t *testing.T) {
	// Rails controller actions are entry points (called by the framework)
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "users_controller.rb")

	code := `class UsersController < ApplicationController
  def index
    @users = User.all
  end

  def show
    @user = User.find(params[:id])
  end

  def create
    @user = User.new(user_params)
    @user.save
  end

  private

  def user_params
    params.require(:user).permit(:name)
  end
end
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithConfidence(0.5))
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Controller actions (index, show, create) should NOT be reported as dead
	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	if deadNames["index"] {
		t.Error("'index' should not be reported as dead (it's a controller action)")
	}
	if deadNames["show"] {
		t.Error("'show' should not be reported as dead (it's a controller action)")
	}
	if deadNames["create"] {
		t.Error("'create' should not be reported as dead (it's a controller action)")
	}
}

func TestGo_DefinitionNamesNotCountedAsUsages(t *testing.T) {
	// Same test for Go - definition names should not count as usages
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "helper.go")

	code := `package main

func unusedHelper() int {
	return 42
}
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

	// 'unusedHelper' should be in definitions but NOT in usages
	if _, exists := result.definitions["unusedHelper"]; !exists {
		t.Error("'unusedHelper' should be in definitions")
	}
	if result.usages["unusedHelper"] {
		t.Error("'unusedHelper' should NOT be in usages (it's only defined, not called)")
	}
}

func TestTypeScript_DecoratedMethodsAreEntryPoints(t *testing.T) {
	// TypeScript methods with decorators (like NestJS @Get) are entry points
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "users.controller.ts")

	code := `import { Controller, Get, Post } from '@nestjs/common';

@Controller('users')
class UsersController {
  @Get()
  findAll() {
    return [];
  }

  @Post()
  create() {
    return {};
  }

  private unusedHelper() {
    return null;
  }
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New(WithConfidence(0.5))
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Decorated methods should NOT be reported as dead
	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	if deadNames["findAll"] {
		t.Error("'findAll' should not be reported as dead (it has @Get decorator)")
	}
	if deadNames["create"] {
		t.Error("'create' should not be reported as dead (it has @Post decorator)")
	}
}

func TestRuby_PunditPolicyMethodsAreEntryPoints(t *testing.T) {
	// Pundit authorization framework calls policy methods dynamically based on
	// controller actions. Methods ending in ? like index?, show?, create? etc.
	// are entry points called by the framework.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "user_policy.rb")

	code := `class UserPolicy < ApplicationPolicy
  def index?
    user.admin?
  end

  def show?
    user.admin? || record.owner == user
  end

  def create?
    user.admin?
  end

  def update_user_roles?
    manage_external_roles? || manage_internal_roles?
  end

  private

  def manage_external_roles?
    permission_checker.can?(:manage_external_roles)
  end

  def manage_internal_roles?
    permission_checker.can?(:manage_internal_roles)
  end

  def helper_method
    nil
  end
end
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Public methods in *_policy.rb files should NOT be reported as dead
	// because they are dynamically called by Pundit
	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	// Public policy methods should not be dead
	if deadNames["index?"] {
		t.Error("'index?' should not be reported as dead (Pundit policy method)")
	}
	if deadNames["show?"] {
		t.Error("'show?' should not be reported as dead (Pundit policy method)")
	}
	if deadNames["create?"] {
		t.Error("'create?' should not be reported as dead (Pundit policy method)")
	}
	if deadNames["update_user_roles?"] {
		t.Error("'update_user_roles?' should not be reported as dead (Pundit policy method)")
	}

	// Private helper methods that are called should not be dead
	if deadNames["manage_external_roles?"] {
		t.Error("'manage_external_roles?' should not be reported as dead (called by update_user_roles?)")
	}
	if deadNames["manage_internal_roles?"] {
		t.Error("'manage_internal_roles?' should not be reported as dead (called by update_user_roles?)")
	}

	// But private methods that aren't called SHOULD be dead
	if !deadNames["helper_method"] {
		t.Error("'helper_method' SHOULD be reported as dead (private and unused)")
	}
}

func TestRuby_GraphQLTypeMethodsAreEntryPoints(t *testing.T) {
	// GraphQL type resolvers are public methods in GraphQL type classes that
	// are called dynamically by the GraphQL framework.
	tmpDir := t.TempDir()
	graphqlDir := filepath.Join(tmpDir, "app", "graphql", "types")
	if err := os.MkdirAll(graphqlDir, 0755); err != nil {
		t.Fatalf("Failed to create graphql directory: %v", err)
	}
	path := filepath.Join(graphqlDir, "order_type.rb")

	code := `class Types::OrderType < Types::BaseObject
  field :id, ID, null: false
  field :status, String, null: false
  field :is_claimed_by_courier, Boolean, null: true

  def is_claimed_by_courier
    object.claimed_courier_order.present?
  end

  def status
    object.status_label
  end

  private

  def internal_helper
    nil
  end
end
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Public methods in GraphQL types are resolvers called by the framework
	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	if deadNames["is_claimed_by_courier"] {
		t.Error("'is_claimed_by_courier' should not be reported as dead (GraphQL resolver)")
	}
	if deadNames["status"] {
		t.Error("'status' should not be reported as dead (GraphQL resolver)")
	}

	// Private helper methods that aren't called SHOULD be dead
	if !deadNames["internal_helper"] {
		t.Error("'internal_helper' SHOULD be reported as dead (private and unused)")
	}
}

func TestJavaScript_DefaultExportsAreEntryPoints(t *testing.T) {
	// Default exports in JavaScript/React are typically page components or
	// module entry points that are imported elsewhere.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "UserProfile.js")

	code := `import React from 'react';

export default function UserProfile(props) {
  return (
    <div>
      <h1>{props.name}</h1>
      {renderDetails(props)}
    </div>
  );
}

function renderDetails(props) {
  return <p>{props.email}</p>;
}

function unusedHelper() {
  return null;
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	// Default exported component should not be dead (it's an entry point)
	if deadNames["UserProfile"] {
		t.Error("'UserProfile' should not be reported as dead (default export)")
	}

	// Functions called by the default export should not be dead
	if deadNames["renderDetails"] {
		t.Error("'renderDetails' should not be reported as dead (called by UserProfile)")
	}

	// Unused helper functions SHOULD be dead
	if !deadNames["unusedHelper"] {
		t.Error("'unusedHelper' SHOULD be reported as dead (not called)")
	}
}

func TestJavaScript_NamedExportsAreEntryPoints(t *testing.T) {
	// Named exports in JavaScript modules are part of the public API
	// and should be treated as entry points.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "utils.js")

	code := `// Utility module with named exports
export function formatDate(date) {
  return date.toISOString();
}

export const API_VERSION = '1.0.0';

export function parseJSON(str) {
  return JSON.parse(str);
}

function internalHelper() {
  return null;
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	// Named exported functions should not be dead
	if deadNames["formatDate"] {
		t.Error("'formatDate' should not be reported as dead (named export)")
	}
	if deadNames["parseJSON"] {
		t.Error("'parseJSON' should not be reported as dead (named export)")
	}

	// Internal helper functions SHOULD be dead if not called
	if !deadNames["internalHelper"] {
		t.Error("'internalHelper' SHOULD be reported as dead (not exported, not called)")
	}
}

func TestGo_InitFunctionsAreEntryPoints(t *testing.T) {
	// Go init() functions are called automatically and should be entry points.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "config.go")

	code := `package config

var settings map[string]string

func init() {
	settings = make(map[string]string)
	loadDefaults()
}

func loadDefaults() {
	settings["timeout"] = "30s"
}

func unusedHelper() string {
	return "unused"
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	// init() should not be dead (entry point)
	if deadNames["init"] {
		t.Error("'init' should not be reported as dead (Go entry point)")
	}

	// Functions called by init should not be dead
	if deadNames["loadDefaults"] {
		t.Error("'loadDefaults' should not be reported as dead (called by init)")
	}

	// Unused helpers SHOULD be dead
	if !deadNames["unusedHelper"] {
		t.Error("'unusedHelper' SHOULD be reported as dead (not called)")
	}
}

func TestGo_HTTPHandlersAreEntryPoints(t *testing.T) {
	// HTTP handlers registered with http.HandleFunc are entry points.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "server.go")

	code := `package main

import "net/http"

func main() {
	http.HandleFunc("/health", healthHandler)
	http.HandleFunc("/api/users", usersHandler)
	http.ListenAndServe(":8080", nil)
}

func healthHandler(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("OK"))
}

func usersHandler(w http.ResponseWriter, r *http.Request) {
	fetchUsers(w, r)
}

func fetchUsers(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("[]"))
}

// unexported function not registered anywhere - should be dead
// (doesn't end with Handler/handler suffix to avoid entry point heuristic)
func unusedFunc(w http.ResponseWriter, r *http.Request) {
	w.Write([]byte("unused"))
}
`
	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	deadNames := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadNames[fn.Name] = true
	}

	// main() should not be dead
	if deadNames["main"] {
		t.Error("'main' should not be reported as dead (Go entry point)")
	}

	// Handlers registered with http.HandleFunc should not be dead (exported = entry point)
	if deadNames["healthHandler"] {
		t.Error("'healthHandler' should not be reported as dead (registered handler)")
	}
	if deadNames["usersHandler"] {
		t.Error("'usersHandler' should not be reported as dead (registered handler)")
	}

	// Functions called by handlers should not be dead (exported = entry point)
	if deadNames["fetchUsers"] {
		t.Error("'fetchUsers' should not be reported as dead (called by usersHandler)")
	}

	// Unexported functions that are NOT used SHOULD be dead
	if !deadNames["unusedFunc"] {
		t.Error("'unusedFunc' SHOULD be reported as dead (unexported, not used)")
	}
}

func TestJavaScript_DefaultExportReferenceIsEntryPoint(t *testing.T) {
	// React pattern: const MyComponent = () => {}; export default MyComponent;
	// The variable MyComponent should be treated as an entry point because it's
	// the default export of the module.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "Component.js")

	code := `import React from 'react';

function helperOne() { return 1; }
function helperTwo() { return 2; }

const MyComponent = props => {
  return <div>{helperOne()}</div>;
};

function unusedHelper() { return 'unused'; }

export default MyComponent;`

	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Collect dead names for easier assertions
	deadFunctions := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadFunctions[fn.Name] = true
	}
	deadVariables := make(map[string]bool)
	for _, v := range analysis.DeadVariables {
		deadVariables[v.Name] = true
	}

	// MyComponent is the default export - should NOT be dead
	if deadVariables["MyComponent"] {
		t.Error("'MyComponent' should not be reported as dead (default export reference)")
	}

	// helperOne is called by MyComponent - should NOT be dead
	if deadFunctions["helperOne"] {
		t.Error("'helperOne' should not be reported as dead (called by default export)")
	}

	// helperTwo is NOT called - SHOULD be dead
	if !deadFunctions["helperTwo"] {
		t.Error("'helperTwo' SHOULD be reported as dead (not called)")
	}

	// unusedHelper is NOT called - SHOULD be dead
	if !deadFunctions["unusedHelper"] {
		t.Error("'unusedHelper' SHOULD be reported as dead (not called)")
	}
}

func TestJavaScript_NestedFunctionsInArrowComponent(t *testing.T) {
	// React pattern: arrow function component with nested function declarations.
	// When the component calls module-level helpers in its JSX return, those calls
	// should be attributed to the component, not to the last nested function.
	// This tests proper function context stack management.
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "Component.js")

	// Using function names that don't trigger entry point heuristics (like "handle" prefix)
	code := `import React from 'react';

// Module-level helpers
function formatPrice(n) { return '$' + n; }
function formatDate(d) { return d.toISOString(); }

const MyComponent = props => {
  // Nested function inside arrow component
  function clickAction() {
    console.log('clicked');
  }

  // Another nested function (unused)
  function submitAction() {
    console.log('submitted');
  }

  // Calls to module-level helpers happen AFTER nested function definitions
  // These should be attributed to MyComponent, not submitAction
  return (
    <div>
      <span>{formatPrice(props.price)}</span>
      <button onClick={clickAction}>Click</button>
    </div>
  );
};

function unusedHelper() { return 'unused'; }

export default MyComponent;`

	if err := os.WriteFile(path, []byte(code), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	a := New()
	defer a.Close()

	analysis, err := a.Analyze(context.Background(), []string{path})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Collect dead names for easier assertions
	deadFunctions := make(map[string]bool)
	for _, fn := range analysis.DeadFunctions {
		deadFunctions[fn.Name] = true
	}

	// formatPrice is called in JSX return of MyComponent - should NOT be dead
	if deadFunctions["formatPrice"] {
		t.Error("'formatPrice' should not be reported as dead (called by MyComponent in JSX)")
	}

	// formatDate is NOT called - SHOULD be dead
	if !deadFunctions["formatDate"] {
		t.Error("'formatDate' SHOULD be reported as dead (not called)")
	}

	// clickAction is used as onClick handler - should NOT be dead
	if deadFunctions["clickAction"] {
		t.Error("'clickAction' should not be reported as dead (used as onClick handler)")
	}

	// submitAction is NOT used - SHOULD be dead
	if !deadFunctions["submitAction"] {
		t.Error("'submitAction' SHOULD be reported as dead (not used)")
	}

	// unusedHelper is NOT called - SHOULD be dead
	if !deadFunctions["unusedHelper"] {
		t.Error("'unusedHelper' SHOULD be reported as dead (not called)")
	}
}
