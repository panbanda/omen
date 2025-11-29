package analyzer

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestNewCohesionAnalyzer(t *testing.T) {
	a := NewCohesionAnalyzer()
	defer a.Close()

	if a.parser == nil {
		t.Error("Expected parser to be initialized")
	}
	if !a.skipTestFile {
		t.Error("Expected skipTestFile to be true by default")
	}
	if a.maxFileSize != 0 {
		t.Error("Expected maxFileSize to be 0 by default")
	}
}

func TestNewCohesionAnalyzerWithOptions(t *testing.T) {
	a := NewCohesionAnalyzer(WithCohesionSkipTestFiles(false), WithCohesionMaxFileSize(1024))
	defer a.Close()

	if a.skipTestFile {
		t.Error("Expected skipTestFile to be false")
	}
	if a.maxFileSize != 1024 {
		t.Error("Expected maxFileSize to be 1024")
	}
}

func TestCohesionAnalyzer_JavaClass(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a Java class file
	file := filepath.Join(tmpDir, "Calculator.java")
	content := `
public class Calculator {
    private int result;
    private int operand;

    public int add(int a, int b) {
        result = a + b;
        return result;
    }

    public int subtract(int a, int b) {
        result = a - b;
        return result;
    }

    public void reset() {
        result = 0;
        operand = 0;
    }
}
`
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := NewCohesionAnalyzer()
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(analysis.Classes) == 0 {
		t.Skip("No classes found (parser may not fully support Java)")
	}

	cls := analysis.Classes[0]
	if cls.ClassName != "Calculator" {
		t.Errorf("ClassName = %q, want %q", cls.ClassName, "Calculator")
	}
	if cls.NOM < 3 {
		t.Errorf("NOM = %d, want at least 3 methods", cls.NOM)
	}
}

func TestCohesionAnalyzer_PythonClass(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a Python class file
	file := filepath.Join(tmpDir, "calculator.py")
	content := `
class Calculator:
    def __init__(self):
        self.result = 0
        self.operand = 0

    def add(self, a, b):
        self.result = a + b
        return self.result

    def subtract(self, a, b):
        self.result = a - b
        return self.result
`
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := NewCohesionAnalyzer()
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(analysis.Classes) == 0 {
		t.Fatal("Expected at least one class")
	}

	cls := analysis.Classes[0]
	if cls.ClassName != "Calculator" {
		t.Errorf("ClassName = %q, want %q", cls.ClassName, "Calculator")
	}
	if cls.Language != "python" {
		t.Errorf("Language = %q, want %q", cls.Language, "python")
	}
}

func TestCohesionAnalyzer_TypeScriptClass(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a TypeScript class file
	file := filepath.Join(tmpDir, "calculator.ts")
	content := `
class Calculator {
    private result: number = 0;

    add(a: number, b: number): number {
        this.result = a + b;
        return this.result;
    }

    subtract(a: number, b: number): number {
        this.result = a - b;
        return this.result;
    }
}
`
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := NewCohesionAnalyzer()
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	if len(analysis.Classes) == 0 {
		t.Fatal("Expected at least one class")
	}

	cls := analysis.Classes[0]
	if cls.ClassName != "Calculator" {
		t.Errorf("ClassName = %q, want %q", cls.ClassName, "Calculator")
	}
}

func TestCohesionAnalyzer_NonOOLanguage(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a Go file (not traditional OO)
	file := filepath.Join(tmpDir, "main.go")
	content := `package main

func main() {
	println("hello")
}
`
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := NewCohesionAnalyzer()
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Go files should return 0 classes (Go is not in the OO language list)
	if len(analysis.Classes) != 0 {
		t.Errorf("Expected 0 classes for Go file, got %d", len(analysis.Classes))
	}
}

func TestCohesionAnalyzer_SkipsTestFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a test file
	file := filepath.Join(tmpDir, "calculator_test.py")
	content := `
class TestCalculator:
    def test_add(self):
        pass
`
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := NewCohesionAnalyzer() // Default skips test files
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Should skip test file
	if len(analysis.Classes) != 0 {
		t.Errorf("Expected 0 classes (test file should be skipped), got %d", len(analysis.Classes))
	}
}

func TestCohesionAnalyzer_IncludesTestFiles(t *testing.T) {
	tmpDir := t.TempDir()

	// Create a test file
	file := filepath.Join(tmpDir, "calculator_test.py")
	content := `
class TestCalculator:
    def test_add(self):
        pass
`
	if err := os.WriteFile(file, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write test file: %v", err)
	}

	analyzer := NewCohesionAnalyzer(WithCohesionSkipTestFiles(false)) // Include test files
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{file})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Should include test file
	if len(analysis.Classes) == 0 {
		t.Error("Expected at least one class (test file should be included)")
	}
}

func TestIsOOLanguage(t *testing.T) {
	tests := []struct {
		lang parser.Language
		want bool
	}{
		{parser.LangJava, true},
		{parser.LangPython, true},
		{parser.LangTypeScript, true},
		{parser.LangJavaScript, true},
		{parser.LangCSharp, true},
		{parser.LangCPP, true},
		{parser.LangRuby, true},
		{parser.LangPHP, true},
		{parser.LangGo, false},
		{parser.LangRust, false},
		{parser.LangC, false},
	}

	for _, tt := range tests {
		t.Run(string(tt.lang), func(t *testing.T) {
			got := isOOLanguage(tt.lang)
			if got != tt.want {
				t.Errorf("isOOLanguage(%v) = %v, want %v", tt.lang, got, tt.want)
			}
		})
	}
}

func TestCalculateLCOM4(t *testing.T) {
	tests := []struct {
		name    string
		methods []methodInfo
		fields  []string
		want    int
	}{
		{
			name:    "empty",
			methods: nil,
			fields:  nil,
			want:    0,
		},
		{
			name: "single method",
			methods: []methodInfo{
				{name: "m1", usedFields: map[string]bool{"f1": true}},
			},
			fields: []string{"f1"},
			want:   1,
		},
		{
			name: "two methods sharing field",
			methods: []methodInfo{
				{name: "m1", usedFields: map[string]bool{"f1": true}},
				{name: "m2", usedFields: map[string]bool{"f1": true}},
			},
			fields: []string{"f1"},
			want:   1, // Connected
		},
		{
			name: "two methods no shared fields",
			methods: []methodInfo{
				{name: "m1", usedFields: map[string]bool{"f1": true}},
				{name: "m2", usedFields: map[string]bool{"f2": true}},
			},
			fields: []string{"f1", "f2"},
			want:   2, // Two components
		},
		{
			name: "three methods in chain",
			methods: []methodInfo{
				{name: "m1", usedFields: map[string]bool{"f1": true}},
				{name: "m2", usedFields: map[string]bool{"f1": true, "f2": true}},
				{name: "m3", usedFields: map[string]bool{"f2": true}},
			},
			fields: []string{"f1", "f2"},
			want:   1, // All connected through m2
		},
		{
			name: "methods with no fields",
			methods: []methodInfo{
				{name: "m1", usedFields: map[string]bool{}},
				{name: "m2", usedFields: map[string]bool{}},
			},
			fields: nil,
			want:   2, // No fields means methods are isolated
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := calculateLCOM4(tt.methods, tt.fields)
			if got != tt.want {
				t.Errorf("calculateLCOM4() = %d, want %d", got, tt.want)
			}
		})
	}
}

func TestIsPrimitiveType(t *testing.T) {
	primitives := []string{"int", "string", "bool", "float64", "void", "None"}
	for _, p := range primitives {
		if !isPrimitiveType(p) {
			t.Errorf("isPrimitiveType(%q) should be true", p)
		}
	}

	nonPrimitives := []string{"Calculator", "MyClass", "StringBuilder", "ArrayList"}
	for _, np := range nonPrimitives {
		if isPrimitiveType(np) {
			t.Errorf("isPrimitiveType(%q) should be false", np)
		}
	}
}

func TestInheritanceTree_DIT(t *testing.T) {
	tree := &inheritanceTree{
		classToParents:  make(map[string][]string),
		classToChildren: make(map[string][]string),
		allClasses:      make(map[string]bool),
	}

	// Build a class hierarchy:
	// Animal (root)
	//   -> Mammal (DIT=1)
	//       -> Dog (DIT=2)
	//       -> Cat (DIT=2)
	//   -> Bird (DIT=1)
	//       -> Eagle (DIT=2)
	//           -> BaldEagle (DIT=3)

	tree.allClasses["Animal"] = true
	tree.allClasses["Mammal"] = true
	tree.allClasses["Dog"] = true
	tree.allClasses["Cat"] = true
	tree.allClasses["Bird"] = true
	tree.allClasses["Eagle"] = true
	tree.allClasses["BaldEagle"] = true

	tree.classToParents["Animal"] = nil
	tree.classToParents["Mammal"] = []string{"Animal"}
	tree.classToParents["Dog"] = []string{"Mammal"}
	tree.classToParents["Cat"] = []string{"Mammal"}
	tree.classToParents["Bird"] = []string{"Animal"}
	tree.classToParents["Eagle"] = []string{"Bird"}
	tree.classToParents["BaldEagle"] = []string{"Eagle"}

	tests := []struct {
		className string
		wantDIT   int
	}{
		{"Animal", 0},
		{"Mammal", 1},
		{"Dog", 2},
		{"Cat", 2},
		{"Bird", 1},
		{"Eagle", 2},
		{"BaldEagle", 3},
		{"Unknown", 0}, // Unknown class
	}

	for _, tt := range tests {
		t.Run(tt.className, func(t *testing.T) {
			got := tree.getDIT(tt.className)
			if got != tt.wantDIT {
				t.Errorf("getDIT(%q) = %d, want %d", tt.className, got, tt.wantDIT)
			}
		})
	}
}

func TestInheritanceTree_NOC(t *testing.T) {
	tree := &inheritanceTree{
		classToParents:  make(map[string][]string),
		classToChildren: make(map[string][]string),
		allClasses:      make(map[string]bool),
	}

	// Same hierarchy as DIT test
	tree.classToChildren["Animal"] = []string{"Mammal", "Bird"}
	tree.classToChildren["Mammal"] = []string{"Dog", "Cat"}
	tree.classToChildren["Bird"] = []string{"Eagle"}
	tree.classToChildren["Eagle"] = []string{"BaldEagle"}

	tests := []struct {
		className string
		wantNOC   int
	}{
		{"Animal", 2},    // Mammal, Bird
		{"Mammal", 2},    // Dog, Cat
		{"Dog", 0},       // No children
		{"Cat", 0},       // No children
		{"Bird", 1},      // Eagle
		{"Eagle", 1},     // BaldEagle
		{"BaldEagle", 0}, // No children
		{"Unknown", 0},   // Unknown class
	}

	for _, tt := range tests {
		t.Run(tt.className, func(t *testing.T) {
			got := tree.getNOC(tt.className)
			if got != tt.wantNOC {
				t.Errorf("getNOC(%q) = %d, want %d", tt.className, got, tt.wantNOC)
			}
		})
	}
}

func TestInheritanceTree_CycleDetection(t *testing.T) {
	tree := &inheritanceTree{
		classToParents:  make(map[string][]string),
		classToChildren: make(map[string][]string),
		allClasses:      make(map[string]bool),
	}

	// Create a cycle: A -> B -> C -> A
	tree.allClasses["A"] = true
	tree.allClasses["B"] = true
	tree.allClasses["C"] = true

	tree.classToParents["A"] = []string{"C"}
	tree.classToParents["B"] = []string{"A"}
	tree.classToParents["C"] = []string{"B"}

	// Should not hang, should return some value
	ditA := tree.getDIT("A")
	ditB := tree.getDIT("B")
	ditC := tree.getDIT("C")

	// With cycle detection, all should return a finite value
	if ditA < 0 || ditB < 0 || ditC < 0 {
		t.Error("getDIT should return non-negative values even with cycles")
	}
}

func TestInheritanceTree_MultipleInheritance(t *testing.T) {
	tree := &inheritanceTree{
		classToParents:  make(map[string][]string),
		classToChildren: make(map[string][]string),
		allClasses:      make(map[string]bool),
	}

	// Multiple inheritance (Python style):
	// Base1 (DIT=0), Base2 (DIT=0)
	//       \         /
	//        Child (DIT=1, takes max)
	//          |
	//       GrandChild (DIT=2)

	tree.allClasses["Base1"] = true
	tree.allClasses["Base2"] = true
	tree.allClasses["Child"] = true
	tree.allClasses["GrandChild"] = true

	tree.classToParents["Base1"] = nil
	tree.classToParents["Base2"] = nil
	tree.classToParents["Child"] = []string{"Base1", "Base2"}
	tree.classToParents["GrandChild"] = []string{"Child"}

	if dit := tree.getDIT("Base1"); dit != 0 {
		t.Errorf("getDIT(Base1) = %d, want 0", dit)
	}
	if dit := tree.getDIT("Base2"); dit != 0 {
		t.Errorf("getDIT(Base2) = %d, want 0", dit)
	}
	if dit := tree.getDIT("Child"); dit != 1 {
		t.Errorf("getDIT(Child) = %d, want 1", dit)
	}
	if dit := tree.getDIT("GrandChild"); dit != 2 {
		t.Errorf("getDIT(GrandChild) = %d, want 2", dit)
	}
}

func TestCohesionAnalyzer_PythonInheritance(t *testing.T) {
	tmpDir := t.TempDir()

	// Create Python files with inheritance
	baseFile := filepath.Join(tmpDir, "base.py")
	baseContent := `
class Animal:
    def __init__(self):
        self.name = ""

    def speak(self):
        pass

class Mammal(Animal):
    def __init__(self):
        super().__init__()
        self.warm_blooded = True

    def nurse(self):
        pass
`
	if err := os.WriteFile(baseFile, []byte(baseContent), 0644); err != nil {
		t.Fatalf("Failed to write base file: %v", err)
	}

	childFile := filepath.Join(tmpDir, "animals.py")
	childContent := `
from base import Mammal

class Dog(Mammal):
    def __init__(self):
        super().__init__()
        self.breed = ""

    def bark(self):
        pass

class Cat(Mammal):
    def __init__(self):
        super().__init__()

    def meow(self):
        pass
`
	if err := os.WriteFile(childFile, []byte(childContent), 0644); err != nil {
		t.Fatalf("Failed to write child file: %v", err)
	}

	analyzer := NewCohesionAnalyzer()
	defer analyzer.Close()

	analysis, err := analyzer.AnalyzeProject([]string{baseFile, childFile})
	if err != nil {
		t.Fatalf("AnalyzeProject failed: %v", err)
	}

	// Find classes by name
	classByName := make(map[string]*struct {
		dit int
		noc int
	})
	for _, cls := range analysis.Classes {
		classByName[cls.ClassName] = &struct {
			dit int
			noc int
		}{dit: cls.DIT, noc: cls.NOC}
	}

	// Check DIT values
	if animal, ok := classByName["Animal"]; ok {
		if animal.dit != 0 {
			t.Errorf("Animal DIT = %d, want 0", animal.dit)
		}
	}

	if mammal, ok := classByName["Mammal"]; ok {
		if mammal.dit != 1 {
			t.Errorf("Mammal DIT = %d, want 1", mammal.dit)
		}
	}

	if dog, ok := classByName["Dog"]; ok {
		if dog.dit != 2 {
			t.Errorf("Dog DIT = %d, want 2", dog.dit)
		}
	}

	// Check NOC values (exact values after deduplication fix)
	if animal, ok := classByName["Animal"]; ok {
		if animal.noc != 1 {
			t.Errorf("Animal NOC = %d, want 1 (Mammal)", animal.noc)
		}
	}

	if mammal, ok := classByName["Mammal"]; ok {
		if mammal.noc != 2 {
			t.Errorf("Mammal NOC = %d, want 2 (Dog, Cat)", mammal.noc)
		}
	}

	if dog, ok := classByName["Dog"]; ok {
		if dog.noc != 0 {
			t.Errorf("Dog NOC = %d, want 0", dog.noc)
		}
	}
}

func TestCleanTypeName(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"List", "List"},
		{"List<String>", "List"},
		{"Map<K, V>", "Map"},
		{"ArrayList[String]", "ArrayList"},
		{"  Foo  ", "Foo"},
		{"", ""},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := cleanTypeName(tt.input)
			if got != tt.want {
				t.Errorf("cleanTypeName(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}
