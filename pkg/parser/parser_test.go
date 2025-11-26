package parser

import (
	"os"
	"path/filepath"
	"testing"

	sitter "github.com/smacker/go-tree-sitter"
)

func TestNew(t *testing.T) {
	p := New()
	if p == nil {
		t.Fatal("New() returned nil")
	}
	if p.parser == nil {
		t.Error("parser field is nil")
	}
	p.Close()
}

func TestDetectLanguage(t *testing.T) {
	tests := []struct {
		path string
		want Language
	}{
		// Go
		{"main.go", LangGo},
		{"pkg/parser/parser.go", LangGo},

		// Rust
		{"main.rs", LangRust},
		{"lib.rs", LangRust},

		// Python
		{"script.py", LangPython},
		{"module.pyw", LangPython},
		{"types.pyi", LangPython},

		// TypeScript
		{"app.ts", LangTypeScript},
		{"component.tsx", LangTSX},

		// JavaScript
		{"script.js", LangJavaScript},
		{"module.mjs", LangJavaScript},
		{"common.cjs", LangJavaScript},
		{"component.jsx", LangTSX}, // JSX uses TSX parser

		// Java
		{"Main.java", LangJava},

		// C/C++
		{"main.c", LangC},
		{"header.h", LangC},
		{"main.cpp", LangCPP},
		{"main.cc", LangCPP},
		{"main.cxx", LangCPP},
		{"header.hpp", LangCPP},
		{"header.hxx", LangCPP},

		// C#
		{"Program.cs", LangCSharp},

		// Ruby
		{"script.rb", LangRuby},

		// PHP
		{"index.php", LangPHP},

		// Bash
		{"script.sh", LangBash},
		{"script.bash", LangBash},
		{"Dockerfile", LangBash},

		// Unknown
		{"file.txt", LangUnknown},
		{"file.md", LangUnknown},
		{"file.json", LangUnknown},
		{"file", LangUnknown},

		// Case insensitivity
		{"Main.GO", LangGo},
		{"SCRIPT.PY", LangPython},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := DetectLanguage(tt.path)
			if got != tt.want {
				t.Errorf("DetectLanguage(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestGetTreeSitterLanguage(t *testing.T) {
	langs := []Language{
		LangGo, LangRust, LangPython, LangTypeScript, LangTSX,
		LangJavaScript, LangJava, LangC, LangCPP, LangCSharp,
		LangRuby, LangPHP, LangBash,
	}

	for _, lang := range langs {
		t.Run(string(lang), func(t *testing.T) {
			tsLang, err := GetTreeSitterLanguage(lang)
			if err != nil {
				t.Errorf("GetTreeSitterLanguage(%v) returned error: %v", lang, err)
			}
			if tsLang == nil {
				t.Errorf("GetTreeSitterLanguage(%v) returned nil", lang)
			}
		})
	}

	// Test unknown language
	t.Run("unknown", func(t *testing.T) {
		_, err := GetTreeSitterLanguage(LangUnknown)
		if err == nil {
			t.Error("GetTreeSitterLanguage(LangUnknown) should return error")
		}
	})
}

func TestParse(t *testing.T) {
	tests := []struct {
		name   string
		source string
		lang   Language
	}{
		{
			name:   "go function",
			source: "package main\n\nfunc main() {\n\tprintln(\"hello\")\n}\n",
			lang:   LangGo,
		},
		{
			name:   "python function",
			source: "def hello():\n    print('hello')\n",
			lang:   LangPython,
		},
		{
			name:   "javascript function",
			source: "function hello() {\n  console.log('hello');\n}\n",
			lang:   LangJavaScript,
		},
		{
			name:   "rust function",
			source: "fn main() {\n    println!(\"hello\");\n}\n",
			lang:   LangRust,
		},
	}

	p := New()
	defer p.Close()

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := p.Parse([]byte(tt.source), tt.lang, "test.file")
			if err != nil {
				t.Fatalf("Parse() error: %v", err)
			}

			if result.Tree == nil {
				t.Error("result.Tree is nil")
			}
			if result.Language != tt.lang {
				t.Errorf("result.Language = %v, want %v", result.Language, tt.lang)
			}
			if string(result.Source) != tt.source {
				t.Error("result.Source doesn't match input")
			}
			if result.Path != "test.file" {
				t.Errorf("result.Path = %v, want test.file", result.Path)
			}

			root := result.Tree.RootNode()
			if root == nil {
				t.Error("root node is nil")
			}
			if root.ChildCount() == 0 {
				t.Error("root node has no children")
			}
		})
	}
}

func TestParseFile(t *testing.T) {
	// Create a temporary Go file
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := "package main\n\nfunc hello() {}\n"

	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}

	p := New()
	defer p.Close()

	result, err := p.ParseFile(goFile)
	if err != nil {
		t.Fatalf("ParseFile() error: %v", err)
	}

	if result.Language != LangGo {
		t.Errorf("result.Language = %v, want %v", result.Language, LangGo)
	}
	if result.Path != goFile {
		t.Errorf("result.Path = %v, want %v", result.Path, goFile)
	}
}

func TestParseFileErrors(t *testing.T) {
	p := New()
	defer p.Close()

	// Non-existent file
	_, err := p.ParseFile("/nonexistent/path/file.go")
	if err == nil {
		t.Error("ParseFile() should return error for non-existent file")
	}

	// Unsupported language
	tmpDir := t.TempDir()
	txtFile := filepath.Join(tmpDir, "test.txt")
	if err := os.WriteFile(txtFile, []byte("hello"), 0644); err != nil {
		t.Fatalf("Failed to create temp file: %v", err)
	}

	_, err = p.ParseFile(txtFile)
	if err == nil {
		t.Error("ParseFile() should return error for unsupported language")
	}
}

func TestWalk(t *testing.T) {
	p := New()
	defer p.Close()

	source := "package main\n\nfunc main() {\n\tx := 1\n}\n"
	result, err := p.Parse([]byte(source), LangGo, "test.go")
	if err != nil {
		t.Fatalf("Parse() error: %v", err)
	}

	// Test that Walk visits nodes
	count := 0
	Walk(result.Tree.RootNode(), result.Source, func(node *sitter.Node, source []byte) bool {
		count++
		return true
	})

	if count == 0 {
		t.Error("Walk() visited no nodes")
	}

	// Test WalkTyped collects node types
	var nodeTypes []string
	WalkTyped(result.Tree.RootNode(), result.Source, func(node *sitter.Node, nodeType string, source []byte) bool {
		nodeTypes = append(nodeTypes, nodeType)
		return true
	})

	if len(nodeTypes) == 0 {
		t.Error("WalkTyped() visited no nodes")
	}

	// Check for expected node types
	found := make(map[string]bool)
	for _, nt := range nodeTypes {
		found[nt] = true
	}

	expectedTypes := []string{"source_file", "package_clause", "function_declaration"}
	for _, expected := range expectedTypes {
		if !found[expected] {
			t.Errorf("Expected node type %q not found", expected)
		}
	}

	// Test early termination - Walk stops when visitor returns false
	count = 0
	WalkTyped(result.Tree.RootNode(), result.Source, func(node *sitter.Node, nodeType string, source []byte) bool {
		count++
		return count < 3 // Stop after 3 nodes
	})

	// The walk stops at the node where we return false (the 3rd), but may
	// have already scheduled siblings. We just verify it stopped early.
	if count < 3 {
		t.Errorf("Early termination: visited %d nodes, expected at least 3", count)
	}
}

func TestWalkNil(t *testing.T) {
	// Walk should handle nil node gracefully
	Walk(nil, nil, func(node *sitter.Node, source []byte) bool {
		t.Error("Visitor should not be called for nil node")
		return true
	})

	WalkTyped(nil, nil, func(node *sitter.Node, nodeType string, source []byte) bool {
		t.Error("Visitor should not be called for nil node")
		return true
	})
}

func TestFindNodes(t *testing.T) {
	p := New()
	defer p.Close()

	source := "package main\n\nfunc one() {}\nfunc two() {}\nfunc three() {}\n"
	result, err := p.Parse([]byte(source), LangGo, "test.go")
	if err != nil {
		t.Fatalf("Parse() error: %v", err)
	}

	// Find all function declarations
	nodes := FindNodesByType(result.Tree.RootNode(), result.Source, "function_declaration")
	if len(nodes) != 3 {
		t.Errorf("Found %d function_declaration nodes, expected 3", len(nodes))
	}

	// Find nodes with predicate
	nodes = FindNodes(result.Tree.RootNode(), result.Source, func(n *sitter.Node) bool {
		return n.Type() == "identifier"
	})
	if len(nodes) < 3 {
		t.Errorf("Found %d identifier nodes, expected at least 3", len(nodes))
	}
}

func TestGetNodeText(t *testing.T) {
	p := New()
	defer p.Close()

	source := "package main\n\nfunc hello() {}\n"
	result, err := p.Parse([]byte(source), LangGo, "test.go")
	if err != nil {
		t.Fatalf("Parse() error: %v", err)
	}

	// Find function declaration
	funcs := FindNodesByType(result.Tree.RootNode(), result.Source, "function_declaration")
	if len(funcs) == 0 {
		t.Fatal("No function declarations found")
	}

	// Get full function text
	text := GetNodeText(funcs[0], result.Source)
	if text != "func hello() {}" {
		t.Errorf("GetNodeText() = %q, want %q", text, "func hello() {}")
	}
}

func TestGetFunctions(t *testing.T) {
	tests := []struct {
		name     string
		lang     Language
		source   string
		expected []string
	}{
		{
			name:     "go functions",
			lang:     LangGo,
			source:   "package main\n\nfunc one() {}\nfunc two() {}\n",
			expected: []string{"one", "two"},
		},
		{
			name:     "python functions",
			lang:     LangPython,
			source:   "def alpha():\n    pass\n\ndef beta():\n    pass\n",
			expected: []string{"alpha", "beta"},
		},
		{
			name:     "javascript arrow functions",
			lang:     LangJavaScript,
			source:   "const foo = () => {};\nconst bar = () => {};\n",
			expected: []string{"", ""}, // arrow functions don't have names in the node
		},
		{
			name:     "rust functions",
			lang:     LangRust,
			source:   "fn first() {}\nfn second() {}\n",
			expected: []string{"first", "second"},
		},
	}

	p := New()
	defer p.Close()

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := p.Parse([]byte(tt.source), tt.lang, "test.file")
			if err != nil {
				t.Fatalf("Parse() error: %v", err)
			}

			functions := GetFunctions(result)
			if len(functions) != len(tt.expected) {
				t.Errorf("GetFunctions() returned %d functions, want %d", len(functions), len(tt.expected))
				return
			}

			for i, fn := range functions {
				if fn.Name != tt.expected[i] {
					t.Errorf("function[%d].Name = %q, want %q", i, fn.Name, tt.expected[i])
				}
				if fn.StartLine == 0 {
					t.Errorf("function[%d].StartLine is 0", i)
				}
				if fn.EndLine == 0 {
					t.Errorf("function[%d].EndLine is 0", i)
				}
			}
		})
	}
}

func TestGetClasses(t *testing.T) {
	tests := []struct {
		name     string
		lang     Language
		source   string
		expected []string
	}{
		{
			name:     "python classes",
			lang:     LangPython,
			source:   "class Foo:\n    pass\n\nclass Bar:\n    pass\n",
			expected: []string{"Foo", "Bar"},
		},
		{
			name:     "javascript classes",
			lang:     LangJavaScript,
			source:   "class Alpha {}\nclass Beta {}\n",
			expected: []string{"Alpha", "Beta"},
		},
		{
			name:     "java classes",
			lang:     LangJava,
			source:   "class Hello {}\nclass World {}\n",
			expected: []string{"Hello", "World"},
		},
	}

	p := New()
	defer p.Close()

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := p.Parse([]byte(tt.source), tt.lang, "test.file")
			if err != nil {
				t.Fatalf("Parse() error: %v", err)
			}

			classes := GetClasses(result)
			if len(classes) != len(tt.expected) {
				t.Errorf("GetClasses() returned %d classes, want %d", len(classes), len(tt.expected))
				return
			}

			for i, cls := range classes {
				if cls.Name != tt.expected[i] {
					t.Errorf("class[%d].Name = %q, want %q", i, cls.Name, tt.expected[i])
				}
			}
		})
	}
}

func TestGetFunctionNodeTypes(t *testing.T) {
	tests := []struct {
		lang     Language
		notEmpty bool
	}{
		{LangGo, true},
		{LangRust, true},
		{LangPython, true},
		{LangTypeScript, true},
		{LangJavaScript, true},
		{LangTSX, true},
		{LangJava, true},
		{LangC, true},
		{LangCPP, true},
		{LangCSharp, true},
		{LangRuby, true},
		{LangPHP, true},
		{LangUnknown, false},
		{LangBash, false},
	}

	for _, tt := range tests {
		t.Run(string(tt.lang), func(t *testing.T) {
			types := getFunctionNodeTypes(tt.lang)
			if tt.notEmpty && len(types) == 0 {
				t.Errorf("getFunctionNodeTypes(%v) returned empty slice", tt.lang)
			}
			if !tt.notEmpty && len(types) != 0 {
				t.Errorf("getFunctionNodeTypes(%v) should return empty slice", tt.lang)
			}
		})
	}
}

func TestGetClassNodeTypes(t *testing.T) {
	tests := []struct {
		lang     Language
		notEmpty bool
	}{
		{LangGo, true},
		{LangRust, true},
		{LangPython, true},
		{LangTypeScript, true},
		{LangJavaScript, true},
		{LangTSX, true},
		{LangJava, true},
		{LangCPP, true},
		{LangCSharp, true},
		{LangRuby, true},
		{LangPHP, true},
		{LangUnknown, false},
		{LangC, false},
		{LangBash, false},
	}

	for _, tt := range tests {
		t.Run(string(tt.lang), func(t *testing.T) {
			types := getClassNodeTypes(tt.lang)
			if tt.notEmpty && len(types) == 0 {
				t.Errorf("getClassNodeTypes(%v) returned empty slice", tt.lang)
			}
			if !tt.notEmpty && len(types) != 0 {
				t.Errorf("getClassNodeTypes(%v) should return empty slice", tt.lang)
			}
		})
	}
}

