package treesitter

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/ast"
)

func TestProviderImplementsInterface(t *testing.T) {
	var _ ast.Provider = (*Provider)(nil)
}

func TestProviderParse(t *testing.T) {
	// Create a temp Go file
	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	content := `package main

import "fmt"

func main() {
	fmt.Println("hello")
}

func helper(x int) int {
	return x * 2
}
`
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	provider := New()
	defer provider.Close()

	file, err := provider.Parse(path)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	// Check path
	if file.Path() != path {
		t.Errorf("Path() = %q, want %q", file.Path(), path)
	}

	// Check language
	if file.Language() != ast.LangGo {
		t.Errorf("Language() = %q, want %q", file.Language(), ast.LangGo)
	}

	// Check functions
	fns := file.Functions()
	if len(fns) != 2 {
		t.Errorf("Functions() returned %d functions, want 2", len(fns))
	}

	fnNames := make(map[string]bool)
	for _, fn := range fns {
		fnNames[fn.Name] = true
	}
	if !fnNames["main"] {
		t.Error("missing function 'main'")
	}
	if !fnNames["helper"] {
		t.Error("missing function 'helper'")
	}
}

func TestProviderParseWithTypes(t *testing.T) {
	provider := New()
	defer provider.Close()

	_, err := provider.ParseWithTypes("any.go")
	if err != ast.ErrTypesUnavailable {
		t.Errorf("ParseWithTypes() error = %v, want ErrTypesUnavailable", err)
	}
}

func TestProviderLanguage(t *testing.T) {
	provider := New()
	defer provider.Close()

	tests := []struct {
		path string
		want ast.Language
	}{
		{"main.go", ast.LangGo},
		{"lib.rs", ast.LangRust},
		{"app.py", ast.LangPython},
		{"index.ts", ast.LangTypeScript},
		{"app.tsx", ast.LangTSX},
		{"main.js", ast.LangJavaScript},
		{"App.java", ast.LangJava},
		{"main.c", ast.LangC},
		{"main.cpp", ast.LangCPP},
		{"Program.cs", ast.LangCSharp},
		{"app.rb", ast.LangRuby},
		{"index.php", ast.LangPHP},
		{"script.sh", ast.LangBash},
		{"unknown.xyz", ast.LangUnknown},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := provider.Language(tt.path)
			if got != tt.want {
				t.Errorf("Language(%q) = %q, want %q", tt.path, got, tt.want)
			}
		})
	}
}

func TestFileImports(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	content := `package main

import (
	"fmt"
	"os"
	mypath "path/filepath"
)

func main() {
	fmt.Println(os.Args)
	mypath.Join("a", "b")
}
`
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	provider := New()
	defer provider.Close()

	file, err := provider.Parse(path)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	imports := file.Imports()
	if len(imports) != 3 {
		t.Errorf("Imports() returned %d imports, want 3", len(imports))
	}

	// Check that imports include expected paths
	paths := make(map[string]string)
	for _, imp := range imports {
		paths[imp.Path] = imp.Alias
	}

	if _, ok := paths["fmt"]; !ok {
		t.Error("missing import 'fmt'")
	}
	if _, ok := paths["os"]; !ok {
		t.Error("missing import 'os'")
	}
	if alias, ok := paths["path/filepath"]; !ok {
		t.Error("missing import 'path/filepath'")
	} else if alias != "mypath" {
		t.Errorf("import 'path/filepath' alias = %q, want 'mypath'", alias)
	}
}

func TestFileCalls(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	content := `package main

import "fmt"

func main() {
	fmt.Println("hello")
	helper(42)
}

func helper(x int) int {
	return x * 2
}
`
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	provider := New()
	defer provider.Close()

	file, err := provider.Parse(path)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	calls := file.Calls()
	if len(calls) < 2 {
		t.Errorf("Calls() returned %d calls, want at least 2", len(calls))
	}

	// Check for expected calls
	callees := make(map[string]bool)
	for _, call := range calls {
		callees[call.Callee] = true
	}

	if !callees["Println"] {
		t.Error("missing call to 'Println'")
	}
	if !callees["helper"] {
		t.Error("missing call to 'helper'")
	}
}

func TestFileSymbols(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	content := `package main

func main() {}
func helper() {}
`
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	provider := New()
	defer provider.Close()

	file, err := provider.Parse(path)
	if err != nil {
		t.Fatalf("Parse failed: %v", err)
	}

	symbols := file.Symbols()
	if len(symbols) != 2 {
		t.Errorf("Symbols() returned %d symbols, want 2", len(symbols))
	}

	// Check that all symbols are functions
	for _, sym := range symbols {
		if sym.Kind != ast.SymbolFunction {
			t.Errorf("symbol %q has kind %q, want %q", sym.Name, sym.Kind, ast.SymbolFunction)
		}
	}
}
