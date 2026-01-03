package repomap

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestRepoMap_IncludesSignatures(t *testing.T) {
	// Setup: Create Go file with typed function signatures
	tmpDir := t.TempDir()

	code := `package main

func Add(a int, b int) int {
	return a + b
}

func Process(data []string, handler func(string) error) (int, error) {
	return 0, nil
}

func NoParams() string {
	return ""
}
`

	mainPath := filepath.Join(tmpDir, "main.go")
	err := os.WriteFile(mainPath, []byte(code), 0644)
	if err != nil {
		t.Fatal(err)
	}

	analyzer := New()
	defer analyzer.Close()

	repoMap, err := analyzer.Analyze(context.Background(), []string{mainPath})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	// Find symbols by name and check signatures
	symbolsByName := make(map[string]Symbol)
	for _, s := range repoMap.Symbols {
		symbolsByName[s.Name] = s
	}

	// Test Add function has typed signature
	if add, ok := symbolsByName["Add"]; ok {
		if add.Signature != "func Add(a int, b int) int" {
			t.Errorf("Expected signature 'func Add(a int, b int) int', got '%s'", add.Signature)
		}
	} else {
		t.Error("Expected to find 'Add' symbol")
	}

	// Test Process function with complex types
	if process, ok := symbolsByName["Process"]; ok {
		expected := "func Process(data []string, handler func(string) error) (int, error)"
		if process.Signature != expected {
			t.Errorf("Expected signature '%s', got '%s'", expected, process.Signature)
		}
	} else {
		t.Error("Expected to find 'Process' symbol")
	}

	// Test NoParams function
	if noParams, ok := symbolsByName["NoParams"]; ok {
		if noParams.Signature != "func NoParams() string" {
			t.Errorf("Expected signature 'func NoParams() string', got '%s'", noParams.Signature)
		}
	} else {
		t.Error("Expected to find 'NoParams' symbol")
	}
}

func TestRepoMap_PythonSignatures(t *testing.T) {
	tmpDir := t.TempDir()

	code := `def calculate(x: int, y: int) -> int:
    return x + y

def process(items: list[str]) -> None:
    pass
`

	mainPath := filepath.Join(tmpDir, "main.py")
	err := os.WriteFile(mainPath, []byte(code), 0644)
	if err != nil {
		t.Fatal(err)
	}

	analyzer := New()
	defer analyzer.Close()

	repoMap, err := analyzer.Analyze(context.Background(), []string{mainPath})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	symbolsByName := make(map[string]Symbol)
	for _, s := range repoMap.Symbols {
		symbolsByName[s.Name] = s
	}

	// Test Python function with type hints
	if calc, ok := symbolsByName["calculate"]; ok {
		expected := "def calculate(x: int, y: int) -> int"
		if calc.Signature != expected {
			t.Errorf("Expected signature '%s', got '%s'", expected, calc.Signature)
		}
	} else {
		t.Error("Expected to find 'calculate' symbol")
	}
}

func TestRepoMap_TypeScriptSignatures(t *testing.T) {
	tmpDir := t.TempDir()

	code := `function greet(name: string): string {
    return "Hello, " + name;
}

function process(items: string[], callback: (item: string) => void): number {
    return items.length;
}
`

	mainPath := filepath.Join(tmpDir, "main.ts")
	err := os.WriteFile(mainPath, []byte(code), 0644)
	if err != nil {
		t.Fatal(err)
	}

	analyzer := New()
	defer analyzer.Close()

	repoMap, err := analyzer.Analyze(context.Background(), []string{mainPath})
	if err != nil {
		t.Fatalf("Analyze failed: %v", err)
	}

	symbolsByName := make(map[string]Symbol)
	for _, s := range repoMap.Symbols {
		symbolsByName[s.Name] = s
	}

	// Test TypeScript function with type annotations
	if greet, ok := symbolsByName["greet"]; ok {
		expected := "function greet(name: string): string"
		if greet.Signature != expected {
			t.Errorf("Expected signature '%s', got '%s'", expected, greet.Signature)
		}
	} else {
		t.Error("Expected to find 'greet' symbol")
	}
}
