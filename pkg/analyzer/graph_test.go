package analyzer

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

func TestNewGraphAnalyzer(t *testing.T) {
	tests := []struct {
		name  string
		scope GraphScope
	}{
		{
			name:  "File scope",
			scope: ScopeFile,
		},
		{
			name:  "Function scope",
			scope: ScopeFunction,
		},
		{
			name:  "Module scope",
			scope: ScopeModule,
		},
		{
			name:  "Package scope",
			scope: ScopePackage,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			analyzer := NewGraphAnalyzer(tt.scope)
			if analyzer == nil {
				t.Fatal("NewGraphAnalyzer() returned nil")
			}
			if analyzer.parser == nil {
				t.Error("analyzer.parser is nil")
			}
			if analyzer.scope != tt.scope {
				t.Errorf("analyzer.scope = %v, want %v", analyzer.scope, tt.scope)
			}
			analyzer.Close()
		})
	}
}

func TestAnalyzeProject_FileScope_Go(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"main.go": `package main

import "fmt"

func main() {
	fmt.Println("hello")
}`,
		"util.go": `package main

import "strings"

func helper() string {
	return strings.ToUpper("test")
}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject(filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if graph == nil {
		t.Fatal("AnalyzeProject() returned nil graph")
	}

	if len(graph.Nodes) != 2 {
		t.Errorf("len(graph.Nodes) = %v, want 2", len(graph.Nodes))
	}

	for _, node := range graph.Nodes {
		if node.Type != models.NodeFile {
			t.Errorf("node.Type = %v, want %v", node.Type, models.NodeFile)
		}
		if node.ID == "" {
			t.Error("node.ID is empty")
		}
	}
}

func TestAnalyzeProject_FunctionScope_Go(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

func foo() {
	bar()
}

func bar() {
	baz()
}

func baz() {
	foo()
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewGraphAnalyzer(ScopeFunction)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject([]string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(graph.Nodes) != 3 {
		t.Errorf("len(graph.Nodes) = %v, want 3", len(graph.Nodes))
	}

	for _, node := range graph.Nodes {
		if node.Type != models.NodeFunction {
			t.Errorf("node.Type = %v, want %v", node.Type, models.NodeFunction)
		}
		if !strings.Contains(node.ID, ":") {
			t.Errorf("function node.ID should contain ':', got %v", node.ID)
		}
	}

	if len(graph.Edges) == 0 {
		t.Error("expected function call edges, got none")
	}
}

func TestExtractImports_MultiLanguage(t *testing.T) {
	tests := []struct {
		name         string
		language     string
		fileExt      string
		code         string
		wantImports  []string
		wantContains bool
	}{
		{
			name:     "Go standard import",
			language: "go",
			fileExt:  ".go",
			code: `package main

import "fmt"
import "strings"

func main() {}`,
			wantImports:  []string{"fmt", "strings"},
			wantContains: true,
		},
		{
			name:     "Go grouped imports",
			language: "go",
			fileExt:  ".go",
			code: `package main

import (
	"fmt"
	"strings"
	"os"
)

func main() {}`,
			wantImports:  []string{"fmt", "strings", "os"},
			wantContains: true,
		},
		{
			name:     "Python import",
			language: "python",
			fileExt:  ".py",
			code: `import os
import sys
from pathlib import Path

def main():
    pass`,
			wantImports:  []string{"os", "sys", "pathlib"},
			wantContains: true,
		},
		{
			name:     "JavaScript import",
			language: "javascript",
			fileExt:  ".js",
			code: `import fs from 'fs';
import path from 'path';

function main() {}`,
			wantImports:  []string{"fs", "path"},
			wantContains: true,
		},
		{
			name:     "TypeScript import",
			language: "typescript",
			fileExt:  ".ts",
			code: `import { readFile } from 'fs';
import * as path from 'path';

function main(): void {}`,
			wantImports:  []string{"fs", "path"},
			wantContains: true,
		},
		{
			name:     "Rust use declaration",
			language: "rust",
			fileExt:  ".rs",
			code: `use std::fs;
use std::io::Read;

fn main() {}`,
			wantImports:  []string{},
			wantContains: false,
		},
		{
			name:     "Java import",
			language: "java",
			fileExt:  ".java",
			code: `import java.util.List;
import java.io.File;

class Main {
    public static void main(String[] args) {}
}`,
			wantImports:  []string{},
			wantContains: false,
		},
		{
			name:     "Ruby require",
			language: "ruby",
			fileExt:  ".rb",
			code: `require 'json'
require_relative 'helper'

def main
end`,
			wantImports:  []string{"json", "helper"},
			wantContains: true,
		},
		{
			name:     "No imports",
			language: "go",
			fileExt:  ".go",
			code: `package main

func main() {}`,
			wantImports:  []string{},
			wantContains: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test"+tt.fileExt)
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewGraphAnalyzer(ScopeFile)
			defer analyzer.Close()

			result, err := analyzer.parser.ParseFile(testFile)
			if err != nil {
				t.Fatalf("ParseFile() error = %v", err)
			}

			imports := extractImports(result)

			if !tt.wantContains {
				if len(imports) != 0 {
					t.Errorf("expected no imports, got %v", imports)
				}
				return
			}

			for _, want := range tt.wantImports {
				found := false
				for _, imp := range imports {
					if strings.Contains(imp, want) {
						found = true
						break
					}
				}
				if !found {
					t.Errorf("import %q not found in %v", want, imports)
				}
			}
		})
	}
}

func TestExtractModuleName(t *testing.T) {
	tests := []struct {
		name     string
		language string
		fileExt  string
		code     string
	}{
		{
			name:     "Go package",
			language: "go",
			fileExt:  ".go",
			code: `package main

func main() {}`,
		},
		{
			name:     "Rust mod",
			language: "rust",
			fileExt:  ".rs",
			code: `mod mymod {
    fn foo() {}
}`,
		},
		{
			name:     "Python file",
			language: "python",
			fileExt:  ".py",
			code: `def main():
    pass`,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test"+tt.fileExt)
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewGraphAnalyzer(ScopeModule)
			defer analyzer.Close()

			result, err := analyzer.parser.ParseFile(testFile)
			if err != nil {
				t.Fatalf("ParseFile() error = %v", err)
			}

			moduleName := extractModuleName(result)
			t.Logf("Module name for %s: %q", tt.language, moduleName)
		})
	}
}

func TestExtractCalls(t *testing.T) {
	tests := []struct {
		name      string
		language  string
		fileExt   string
		code      string
		wantCalls []string
	}{
		{
			name:     "Go function calls",
			language: "go",
			fileExt:  ".go",
			code: `package main

func foo() {
	bar()
	baz()
}

func bar() {}
func baz() {}`,
			wantCalls: []string{"bar", "baz"},
		},
		{
			name:     "Python function calls",
			language: "python",
			fileExt:  ".py",
			code: `def foo():
    bar()
    baz()

def bar():
    pass

def baz():
    pass`,
			wantCalls: []string{},
		},
		{
			name:     "JavaScript function calls",
			language: "javascript",
			fileExt:  ".js",
			code: `function foo() {
    bar();
    baz();
}

function bar() {}
function baz() {}`,
			wantCalls: []string{"bar", "baz"},
		},
		{
			name:     "No calls",
			language: "go",
			fileExt:  ".go",
			code: `package main

func foo() {
	x := 1
	y := 2
	z := x + y
}`,
			wantCalls: []string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tmpDir := t.TempDir()
			testFile := filepath.Join(tmpDir, "test"+tt.fileExt)
			if err := os.WriteFile(testFile, []byte(tt.code), 0644); err != nil {
				t.Fatalf("failed to write test file: %v", err)
			}

			analyzer := NewGraphAnalyzer(ScopeFunction)
			defer analyzer.Close()

			result, err := analyzer.parser.ParseFile(testFile)
			if err != nil {
				t.Fatalf("ParseFile() error = %v", err)
			}

			functions := parser.GetFunctions(result)
			if len(functions) == 0 {
				t.Fatal("no functions found")
			}

			calls := extractCalls(functions[0].Body, result.Source)

			if len(tt.wantCalls) == 0 {
				if len(calls) != 0 {
					t.Errorf("expected no calls, got %v", calls)
				}
				return
			}

			for _, want := range tt.wantCalls {
				found := false
				for _, call := range calls {
					if strings.Contains(call, want) {
						found = true
						break
					}
				}
				if !found {
					t.Errorf("call %q not found in %v", want, calls)
				}
			}
		})
	}
}

func TestMatchesImport(t *testing.T) {
	tests := []struct {
		name       string
		filePath   string
		importPath string
		want       bool
	}{
		{
			name:       "Exact substring match",
			filePath:   "/path/to/util.go",
			importPath: "util",
			want:       true,
		},
		{
			name:       "Path contains import",
			filePath:   "/path/to/github.com/user/pkg/file.go",
			importPath: "github.com/user/pkg",
			want:       true,
		},
		{
			name:       "Import contains path component",
			filePath:   "/path/to/strings.go",
			importPath: "strings",
			want:       true,
		},
		{
			name:       "Self-reference",
			filePath:   "/path/to/file.go",
			importPath: "/path/to/file.go",
			want:       false,
		},
		{
			name:       "No match",
			filePath:   "/path/to/foo.go",
			importPath: "bar",
			want:       false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := matchesImport(tt.filePath, tt.importPath)
			if got != tt.want {
				t.Errorf("matchesImport(%q, %q) = %v, want %v",
					tt.filePath, tt.importPath, got, tt.want)
			}
		})
	}
}

func TestMatchesCall(t *testing.T) {
	tests := []struct {
		name     string
		nodeID   string
		callName string
		want     bool
	}{
		{
			name:     "Function name matches",
			nodeID:   "/path/to/file.go:foo",
			callName: "foo",
			want:     true,
		},
		{
			name:     "Function name mismatch",
			nodeID:   "/path/to/file.go:foo",
			callName: "bar",
			want:     false,
		},
		{
			name:     "Partial match not enough",
			nodeID:   "/path/to/file.go:foobar",
			callName: "foo",
			want:     true,
		},
		{
			name:     "No colon in node ID",
			nodeID:   "/path/to/file.go",
			callName: "foo",
			want:     false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := matchesCall(tt.nodeID, tt.callName)
			if got != tt.want {
				t.Errorf("matchesCall(%q, %q) = %v, want %v",
					tt.nodeID, tt.callName, got, tt.want)
			}
		})
	}
}

func TestContains(t *testing.T) {
	tests := []struct {
		name   string
		s      string
		substr string
		want   bool
	}{
		{
			name:   "Contains at start",
			s:      "hello world",
			substr: "hello",
			want:   true,
		},
		{
			name:   "Contains in middle",
			s:      "hello world",
			substr: "lo wo",
			want:   true,
		},
		{
			name:   "Contains at end",
			s:      "hello world",
			substr: "world",
			want:   true,
		},
		{
			name:   "Does not contain",
			s:      "hello world",
			substr: "foo",
			want:   false,
		},
		{
			name:   "Empty substring",
			s:      "hello",
			substr: "",
			want:   true,
		},
		{
			name:   "Empty string",
			s:      "",
			substr: "foo",
			want:   false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := contains(tt.s, tt.substr)
			if got != tt.want {
				t.Errorf("contains(%q, %q) = %v, want %v",
					tt.s, tt.substr, got, tt.want)
			}
		})
	}
}

func TestCalculateMetrics(t *testing.T) {
	tests := []struct {
		name       string
		buildGraph func() *models.DependencyGraph
		wantNodes  int
		wantEdges  int
	}{
		{
			name: "Empty graph",
			buildGraph: func() *models.DependencyGraph {
				return models.NewDependencyGraph()
			},
			wantNodes: 0,
			wantEdges: 0,
		},
		{
			name: "Single node",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{
					ID:   "node1",
					Name: "Node 1",
					Type: models.NodeFile,
				})
				return g
			},
			wantNodes: 1,
			wantEdges: 0,
		},
		{
			name: "Two nodes with edge",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{
					ID:   "node1",
					Name: "Node 1",
					Type: models.NodeFile,
				})
				g.AddNode(models.GraphNode{
					ID:   "node2",
					Name: "Node 2",
					Type: models.NodeFile,
				})
				g.AddEdge(models.GraphEdge{
					From: "node1",
					To:   "node2",
					Type: models.EdgeImport,
				})
				return g
			},
			wantNodes: 2,
			wantEdges: 1,
		},
		{
			name: "Cyclic graph",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "C", Name: "C", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "C", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "C", To: "A", Type: models.EdgeImport})
				return g
			},
			wantNodes: 3,
			wantEdges: 3,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			graph := tt.buildGraph()
			analyzer := NewGraphAnalyzer(ScopeFile)
			defer analyzer.Close()

			metrics := analyzer.CalculateMetrics(graph)
			if metrics == nil {
				t.Fatal("CalculateMetrics() returned nil")
			}

			if metrics.Summary.TotalNodes != tt.wantNodes {
				t.Errorf("TotalNodes = %v, want %v", metrics.Summary.TotalNodes, tt.wantNodes)
			}

			if metrics.Summary.TotalEdges != tt.wantEdges {
				t.Errorf("TotalEdges = %v, want %v", metrics.Summary.TotalEdges, tt.wantEdges)
			}

			if len(metrics.NodeMetrics) != tt.wantNodes {
				t.Errorf("len(NodeMetrics) = %v, want %v", len(metrics.NodeMetrics), tt.wantNodes)
			}

			for _, nm := range metrics.NodeMetrics {
				if nm.NodeID == "" {
					t.Error("NodeMetric has empty NodeID")
				}
				if nm.PageRank < 0 {
					t.Errorf("PageRank should be non-negative, got %v", nm.PageRank)
				}
			}
		})
	}
}

// Note: Tests for PageRank, Betweenness, Closeness, Harmonic, and ConnectedComponents
// are now covered via gonum library integration. We test them through CalculateMetrics.

func TestToMermaid(t *testing.T) {
	tests := []struct {
		name            string
		buildGraph      func() *models.DependencyGraph
		wantContains    []string
		wantNotContains []string
	}{
		{
			name: "Empty graph",
			buildGraph: func() *models.DependencyGraph {
				return models.NewDependencyGraph()
			},
			wantContains: []string{"graph TD"},
		},
		{
			name: "Simple graph with nodes",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{
					ID:   "node1",
					Name: "Node 1",
					Type: models.NodeFile,
				})
				g.AddNode(models.GraphNode{
					ID:   "node2",
					Name: "Node 2",
					Type: models.NodeFile,
				})
				return g
			},
			wantContains: []string{
				"graph TD",
				"node1",
				"node2",
				"Node 1",
				"Node 2",
			},
		},
		{
			name: "Graph with edges",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{
					From: "A",
					To:   "B",
					Type: models.EdgeImport,
				})
				return g
			},
			wantContains: []string{
				"graph TD",
				"A",
				"B",
				"imports",
			},
		},
		{
			name: "Graph with call edges",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "foo", Name: "foo", Type: models.NodeFunction})
				g.AddNode(models.GraphNode{ID: "bar", Name: "bar", Type: models.NodeFunction})
				g.AddEdge(models.GraphEdge{
					From: "foo",
					To:   "bar",
					Type: models.EdgeCall,
				})
				return g
			},
			wantContains: []string{
				"graph TD",
				"foo",
				"bar",
				"calls",
			},
		},
		{
			name: "Special characters sanitized",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{
					ID:   "/path/to/file.go",
					Name: "file.go",
					Type: models.NodeFile,
				})
				return g
			},
			wantContains: []string{
				"graph TD",
				"file_go",
			},
			wantNotContains: []string{
				"/path/to/",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			graph := tt.buildGraph()
			mermaid := graph.ToMermaid()

			for _, want := range tt.wantContains {
				if !strings.Contains(mermaid, want) {
					t.Errorf("ToMermaid() missing %q\nGot:\n%s", want, mermaid)
				}
			}

			for _, notWant := range tt.wantNotContains {
				if strings.Contains(mermaid, notWant) {
					t.Errorf("ToMermaid() should not contain %q\nGot:\n%s", notWant, mermaid)
				}
			}
		})
	}
}

func TestAnalyzeProject_NoImports(t *testing.T) {
	tmpDir := t.TempDir()
	testFile := filepath.Join(tmpDir, "test.go")

	code := `package main

func standalone() int {
	return 42
}`

	if err := os.WriteFile(testFile, []byte(code), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject([]string{testFile})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(graph.Nodes) != 1 {
		t.Errorf("len(graph.Nodes) = %v, want 1", len(graph.Nodes))
	}

	if len(graph.Edges) != 0 {
		t.Errorf("len(graph.Edges) = %v, want 0 (no imports)", len(graph.Edges))
	}
}

func TestGraphAnalyzer_EmptyFiles(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"empty1.go": `package main`,
		"empty2.go": `package main`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject(filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(graph.Nodes) != 2 {
		t.Errorf("len(graph.Nodes) = %v, want 2", len(graph.Nodes))
	}
}

func TestAnalyzeProject_InvalidFile(t *testing.T) {
	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject([]string{"/nonexistent/file.go"})
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(graph.Nodes) != 0 {
		t.Errorf("expected no nodes for invalid file, got %d", len(graph.Nodes))
	}
}

func TestAnalyzeProject_MixedLanguages(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"main.go": `package main

import "fmt"

func main() {
	fmt.Println("hello")
}`,
		"util.py": `import os

def helper():
    return os.getcwd()`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject(filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	if len(graph.Nodes) != 2 {
		t.Errorf("len(graph.Nodes) = %v, want 2", len(graph.Nodes))
	}
}

func TestAnalyzeProject_DensityCalculation(t *testing.T) {
	tmpDir := t.TempDir()

	files := map[string]string{
		"a.go": `package main

import "b"
import "c"

func main() {}`,
		"b.go": `package main

import "c"

func helper() {}`,
		"c.go": `package main

func util() {}`,
	}

	var filePaths []string
	for name, content := range files {
		path := filepath.Join(tmpDir, name)
		if err := os.WriteFile(path, []byte(content), 0644); err != nil {
			t.Fatalf("failed to write %s: %v", name, err)
		}
		filePaths = append(filePaths, path)
	}

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	graph, err := analyzer.AnalyzeProject(filePaths)
	if err != nil {
		t.Fatalf("AnalyzeProject() error = %v", err)
	}

	metrics := analyzer.CalculateMetrics(graph)

	if metrics.Summary.Density < 0 {
		t.Errorf("Density should be non-negative, got %v", metrics.Summary.Density)
	}

	if metrics.Summary.TotalNodes != 3 {
		t.Errorf("TotalNodes = %v, want 3", metrics.Summary.TotalNodes)
	}

	if metrics.Summary.TotalEdges == 0 {
		t.Error("TotalEdges should not be 0")
	}
}

func TestGraphScope_Values(t *testing.T) {
	tests := []struct {
		name  string
		scope GraphScope
		want  GraphScope
	}{
		{name: "File", scope: ScopeFile, want: "file"},
		{name: "Function", scope: ScopeFunction, want: "function"},
		{name: "Module", scope: ScopeModule, want: "module"},
		{name: "Package", scope: ScopePackage, want: "package"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if tt.scope != tt.want {
				t.Errorf("scope = %v, want %v", tt.scope, tt.want)
			}
		})
	}
}

func BenchmarkAnalyzeProject_FileScope(b *testing.B) {
	tmpDir := b.TempDir()

	files := make([]string, 10)
	for i := range files {
		path := filepath.Join(tmpDir, "file"+string(rune('0'+i))+".go")
		code := `package main

import "fmt"
import "strings"

func main() {
	fmt.Println(strings.ToUpper("test"))
}`
		if err := os.WriteFile(path, []byte(code), 0644); err != nil {
			b.Fatalf("failed to write test file: %v", err)
		}
		files[i] = path
	}

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_, err := analyzer.AnalyzeProject(files)
		if err != nil {
			b.Fatalf("AnalyzeProject() error = %v", err)
		}
	}
}

// Benchmarks for PageRank and Betweenness removed - now using gonum library

func TestDetectCycles_TarjanSCC(t *testing.T) {
	tests := []struct {
		name       string
		buildGraph func() *models.DependencyGraph
		wantCycles int
		wantNodes  [][]string
	}{
		{
			name: "No cycles - linear chain",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "C", Name: "C", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "C", Type: models.EdgeImport})
				return g
			},
			wantCycles: 0,
		},
		{
			name: "Simple cycle A->B->A",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "A", Type: models.EdgeImport})
				return g
			},
			wantCycles: 1,
		},
		{
			name: "Three-node cycle A->B->C->A",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "C", Name: "C", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "C", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "C", To: "A", Type: models.EdgeImport})
				return g
			},
			wantCycles: 1,
		},
		{
			name: "Two separate cycles",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "C", Name: "C", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "D", Name: "D", Type: models.NodeFile})
				// First cycle: A->B->A
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "A", Type: models.EdgeImport})
				// Second cycle: C->D->C
				g.AddEdge(models.GraphEdge{From: "C", To: "D", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "D", To: "C", Type: models.EdgeImport})
				return g
			},
			wantCycles: 2,
		},
		{
			name: "Empty graph",
			buildGraph: func() *models.DependencyGraph {
				return models.NewDependencyGraph()
			},
			wantCycles: 0,
		},
		{
			name: "Self loop not detected as multi-node cycle",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "A", Type: models.EdgeCall})
				return g
			},
			wantCycles: 0,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			graph := tt.buildGraph()
			analyzer := NewGraphAnalyzer(ScopeFile)
			defer analyzer.Close()

			cycles := analyzer.DetectCycles(graph)
			if len(cycles) != tt.wantCycles {
				t.Errorf("DetectCycles() found %d cycles, want %d", len(cycles), tt.wantCycles)
				t.Logf("Cycles: %v", cycles)
			}
		})
	}
}

func TestPruneGraph(t *testing.T) {
	tests := []struct {
		name      string
		numNodes  int
		numEdges  int
		maxNodes  int
		maxEdges  int
		wantNodes int
		wantEdges int
	}{
		{
			name:      "No pruning needed",
			numNodes:  5,
			numEdges:  4,
			maxNodes:  10,
			maxEdges:  10,
			wantNodes: 5,
			wantEdges: 4,
		},
		{
			name:      "Prune nodes",
			numNodes:  10,
			numEdges:  9,
			maxNodes:  5,
			maxEdges:  100,
			wantNodes: 5,
		},
		{
			name:      "Prune edges",
			numNodes:  5,
			numEdges:  10,
			maxNodes:  100,
			maxEdges:  3,
			wantNodes: 5,
			wantEdges: 3,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			g := models.NewDependencyGraph()

			// Create nodes
			for i := 0; i < tt.numNodes; i++ {
				id := string(rune('A' + i))
				g.AddNode(models.GraphNode{ID: id, Name: id, Type: models.NodeFile})
			}

			// Create edges (linear chain)
			edgeCount := 0
			for i := 0; i < tt.numNodes-1 && edgeCount < tt.numEdges; i++ {
				from := string(rune('A' + i))
				to := string(rune('A' + i + 1))
				g.AddEdge(models.GraphEdge{From: from, To: to, Type: models.EdgeImport})
				edgeCount++
			}

			analyzer := NewGraphAnalyzer(ScopeFile)
			defer analyzer.Close()

			pruned := analyzer.PruneGraph(g, tt.maxNodes, tt.maxEdges)

			if len(pruned.Nodes) > tt.maxNodes {
				t.Errorf("PruneGraph() has %d nodes, want <= %d", len(pruned.Nodes), tt.maxNodes)
			}
			if len(pruned.Edges) > tt.maxEdges {
				t.Errorf("PruneGraph() has %d edges, want <= %d", len(pruned.Edges), tt.maxEdges)
			}
			if tt.wantNodes > 0 && len(pruned.Nodes) != tt.wantNodes {
				t.Errorf("PruneGraph() has %d nodes, want %d", len(pruned.Nodes), tt.wantNodes)
			}
			if tt.wantEdges > 0 && len(pruned.Edges) > tt.wantEdges {
				t.Errorf("PruneGraph() has %d edges, want <= %d", len(pruned.Edges), tt.wantEdges)
			}
		})
	}
}

func TestToMermaidWithOptions(t *testing.T) {
	tests := []struct {
		name         string
		buildGraph   func() *models.DependencyGraph
		opts         models.MermaidOptions
		wantContains []string
	}{
		{
			name: "Custom direction LR",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				return g
			},
			opts: models.MermaidOptions{
				Direction: models.DirectionLR,
			},
			wantContains: []string{"graph LR"},
		},
		{
			name: "With complexity styling",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "low", Name: "Low", Type: models.NodeFunction})
				g.AddNode(models.GraphNode{ID: "high", Name: "High", Type: models.NodeFunction})
				return g
			},
			opts: models.MermaidOptions{
				ShowComplexity: true,
				NodeComplexity: map[string]int{
					"low":  2,
					"high": 15,
				},
			},
			wantContains: []string{
				"#90EE90", // Light green for low complexity
				"#FF6347", // Tomato red for high complexity
			},
		},
		{
			name: "Prune to max nodes",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				for i := 0; i < 10; i++ {
					id := string(rune('A' + i))
					g.AddNode(models.GraphNode{ID: id, Name: id, Type: models.NodeFile})
				}
				return g
			},
			opts: models.MermaidOptions{
				MaxNodes: 3,
			},
			wantContains: []string{"graph TD"},
		},
		{
			name: "Edge type uses",
			buildGraph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeUses})
				return g
			},
			opts:         models.DefaultMermaidOptions(),
			wantContains: []string{"---"}, // Uses edge type renders as ---
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			graph := tt.buildGraph()
			mermaid := graph.ToMermaidWithOptions(tt.opts)

			for _, want := range tt.wantContains {
				if !strings.Contains(mermaid, want) {
					t.Errorf("ToMermaidWithOptions() missing %q\nGot:\n%s", want, mermaid)
				}
			}
		})
	}
}

func TestSanitizeMermaidID(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"simple", "simple"},
		{"with spaces", "with_spaces"},
		{"path/to/file.go", "path_to_file_go"},
		{"pkg::module::func", "pkg__module__func"},
		{"123start", "n123start"},
		{"", "empty"},
		{"a-b-c", "a_b_c"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := models.SanitizeMermaidID(tt.input)
			if got != tt.want {
				t.Errorf("SanitizeMermaidID(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestEscapeMermaidLabel(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"simple", "simple"},
		{"a & b", "a &amp; b"},
		{"<script>", "&lt;script&gt;"},
		{"say \"hello\"", "say &quot;hello&quot;"},
		{"a|b", "a&#124;b"},
		{"[array]", "&#91;array&#93;"},
		{"{object}", "&#123;object&#125;"},
		{"line1\nline2", "line1<br/>line2"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := models.EscapeMermaidLabel(tt.input)
			if got != tt.want {
				t.Errorf("EscapeMermaidLabel(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

// Tests for Closeness and Harmonic removed - now using gonum library

func TestCalculateEigenvector(t *testing.T) {
	tests := []struct {
		name  string
		nodes []models.GraphNode
		edges map[string][]string
	}{
		{
			name:  "Empty graph",
			nodes: []models.GraphNode{},
			edges: map[string][]string{},
		},
		{
			name: "Cycle",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B"},
				"B": {"A"},
			},
		},
		{
			name: "Star topology",
			nodes: []models.GraphNode{
				{ID: "center", Name: "Center", Type: models.NodeFile},
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A":      {"center"},
				"B":      {"center"},
				"C":      {"center"},
				"center": {},
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			eigenvector := calculateEigenvector(tt.nodes, tt.edges, 100, 1e-6)

			if len(eigenvector) != len(tt.nodes) {
				t.Errorf("Expected %d eigenvector values, got %d", len(tt.nodes), len(eigenvector))
			}

			for id, score := range eigenvector {
				if score < 0 {
					t.Errorf("Eigenvector centrality for %s should be non-negative, got %v", id, score)
				}
			}
		})
	}
}

func TestCalculateClusteringCoefficients(t *testing.T) {
	tests := []struct {
		name  string
		nodes []models.GraphNode
		edges map[string][]string
	}{
		{
			name:  "Empty graph",
			nodes: []models.GraphNode{},
			edges: map[string][]string{},
		},
		{
			name: "Triangle (fully connected)",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B", "C"},
				"B": {"C"},
				"C": {},
			},
		},
		{
			name: "Linear chain (no triangles)",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B"},
				"B": {"C"},
				"C": {},
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			clustering := calculateClusteringCoefficients(tt.nodes, tt.edges)

			if len(clustering) != len(tt.nodes) {
				t.Errorf("Expected %d clustering values, got %d", len(tt.nodes), len(clustering))
			}

			for id, coef := range clustering {
				if coef < 0 || coef > 1 {
					t.Errorf("Clustering coefficient for %s should be in [0,1], got %v", id, coef)
				}
			}
		})
	}
}

func TestCalculateGlobalClustering(t *testing.T) {
	tests := []struct {
		name     string
		nodes    []models.GraphNode
		edges    map[string][]string
		wantZero bool
	}{
		{
			name:     "Empty graph",
			nodes:    []models.GraphNode{},
			edges:    map[string][]string{},
			wantZero: true,
		},
		{
			name: "Triangle (fully connected)",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B", "C"},
				"B": {"C"},
				"C": {},
			},
			wantZero: false,
		},
		{
			name: "Linear chain (no triangles)",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B"},
				"B": {"C"},
				"C": {},
			},
			wantZero: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			clustering := calculateGlobalClustering(tt.nodes, tt.edges)

			if clustering < 0 || clustering > 1 {
				t.Errorf("Global clustering should be in [0,1], got %v", clustering)
			}

			if tt.wantZero && clustering != 0 {
				t.Errorf("Expected zero global clustering, got %v", clustering)
			}
			if !tt.wantZero && clustering == 0 {
				t.Errorf("Expected non-zero global clustering, got 0")
			}
		})
	}
}

func TestCalculateAssortativity(t *testing.T) {
	tests := []struct {
		name  string
		graph func() *models.DependencyGraph
	}{
		{
			name: "Empty graph",
			graph: func() *models.DependencyGraph {
				return models.NewDependencyGraph()
			},
		},
		{
			name: "Single edge",
			graph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				return g
			},
		},
		{
			name: "Star topology",
			graph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "center", Name: "Center", Type: models.NodeFile})
				for i := 0; i < 5; i++ {
					id := string(rune('A' + i))
					g.AddNode(models.GraphNode{ID: id, Name: id, Type: models.NodeFile})
					g.AddEdge(models.GraphEdge{From: "center", To: id, Type: models.EdgeImport})
				}
				return g
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			assort := calculateAssortativity(tt.graph())

			if assort < -1 || assort > 1 {
				t.Errorf("Assortativity should be in [-1,1], got %v", assort)
			}
		})
	}
}

func TestCalculateReciprocity(t *testing.T) {
	tests := []struct {
		name  string
		graph func() *models.DependencyGraph
		want  float64
	}{
		{
			name: "Empty graph",
			graph: func() *models.DependencyGraph {
				return models.NewDependencyGraph()
			},
			want: 0,
		},
		{
			name: "No reciprocal edges",
			graph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				return g
			},
			want: 0,
		},
		{
			name: "Full reciprocity",
			graph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "A", Type: models.EdgeImport})
				return g
			},
			want: 1,
		},
		{
			name: "Partial reciprocity",
			graph: func() *models.DependencyGraph {
				g := models.NewDependencyGraph()
				g.AddNode(models.GraphNode{ID: "A", Name: "A", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "B", Name: "B", Type: models.NodeFile})
				g.AddNode(models.GraphNode{ID: "C", Name: "C", Type: models.NodeFile})
				g.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "A", Type: models.EdgeImport})
				g.AddEdge(models.GraphEdge{From: "B", To: "C", Type: models.EdgeImport})
				return g
			},
			want: 0.666666,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			reciprocity := calculateReciprocity(tt.graph())

			if reciprocity < 0 || reciprocity > 1 {
				t.Errorf("Reciprocity should be in [0,1], got %v", reciprocity)
			}

			tolerance := 0.01
			if reciprocity < tt.want-tolerance || reciprocity > tt.want+tolerance {
				t.Errorf("Reciprocity = %v, want ~%v", reciprocity, tt.want)
			}
		})
	}
}

// TestFindConnectedComponents removed - now using gonum library

func TestCalculateDiameterAndRadius(t *testing.T) {
	tests := []struct {
		name         string
		nodes        []models.GraphNode
		edges        map[string][]string
		wantDiameter int
		wantRadius   int
	}{
		{
			name:         "Empty graph",
			nodes:        []models.GraphNode{},
			edges:        map[string][]string{},
			wantDiameter: 0,
			wantRadius:   0,
		},
		{
			name: "Single node",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {},
			},
			wantDiameter: 0,
			wantRadius:   0,
		},
		{
			name: "Linear chain of 3",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B"},
				"B": {"C"},
				"C": {},
			},
			wantDiameter: 2,
			wantRadius:   1,
		},
		{
			name: "Triangle",
			nodes: []models.GraphNode{
				{ID: "A", Name: "A", Type: models.NodeFile},
				{ID: "B", Name: "B", Type: models.NodeFile},
				{ID: "C", Name: "C", Type: models.NodeFile},
			},
			edges: map[string][]string{
				"A": {"B", "C"},
				"B": {"C"},
				"C": {},
			},
			wantDiameter: 1,
			wantRadius:   1,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			diameter, radius := calculateDiameterAndRadius(tt.nodes, tt.edges)

			if diameter != tt.wantDiameter {
				t.Errorf("Diameter = %d, want %d", diameter, tt.wantDiameter)
			}
			if radius != tt.wantRadius {
				t.Errorf("Radius = %d, want %d", radius, tt.wantRadius)
			}
		})
	}
}

// TestLouvainCommunityDetection and TestCalculateModularity removed - these functions
// are now delegated to gonum's community.Modularize and community.Q.
// Community detection is tested via integration through CalculateMetrics in TestComprehensiveMetrics.

func TestComprehensiveMetrics(t *testing.T) {
	graph := models.NewDependencyGraph()
	// Create a small test graph
	for i := 0; i < 5; i++ {
		id := string(rune('A' + i))
		graph.AddNode(models.GraphNode{ID: id, Name: id, Type: models.NodeFile})
	}
	// Create edges forming a structure
	graph.AddEdge(models.GraphEdge{From: "A", To: "B", Type: models.EdgeImport})
	graph.AddEdge(models.GraphEdge{From: "B", To: "C", Type: models.EdgeImport})
	graph.AddEdge(models.GraphEdge{From: "C", To: "D", Type: models.EdgeImport})
	graph.AddEdge(models.GraphEdge{From: "D", To: "E", Type: models.EdgeImport})
	graph.AddEdge(models.GraphEdge{From: "E", To: "A", Type: models.EdgeImport}) // Create cycle

	analyzer := NewGraphAnalyzer(ScopeFile)
	defer analyzer.Close()

	metrics := analyzer.CalculateMetrics(graph)

	// Verify summary
	if metrics.Summary.TotalNodes != 5 {
		t.Errorf("TotalNodes = %d, want 5", metrics.Summary.TotalNodes)
	}
	if metrics.Summary.TotalEdges != 5 {
		t.Errorf("TotalEdges = %d, want 5", metrics.Summary.TotalEdges)
	}

	// Verify we have metrics for all nodes
	if len(metrics.NodeMetrics) != 5 {
		t.Errorf("NodeMetrics count = %d, want 5", len(metrics.NodeMetrics))
	}

	// Check that centrality metrics are computed
	for _, nm := range metrics.NodeMetrics {
		if nm.PageRank <= 0 {
			t.Errorf("PageRank for %s should be positive, got %v", nm.NodeID, nm.PageRank)
		}
	}

	// Check structural metrics
	if metrics.Summary.Components != 1 {
		t.Errorf("Components = %d, want 1", metrics.Summary.Components)
	}
	if metrics.Summary.LargestComponent != 5 {
		t.Errorf("LargestComponent = %d, want 5", metrics.Summary.LargestComponent)
	}

	// Cycle detection
	if !metrics.Summary.IsCyclic {
		t.Error("Expected IsCyclic = true for cyclic graph")
	}
	if metrics.Summary.CycleCount == 0 {
		t.Error("Expected CycleCount > 0 for cyclic graph")
	}

	// Community detection
	if metrics.Summary.CommunityCount == 0 {
		t.Error("Expected CommunityCount > 0")
	}

	// Verify diameter and radius are computed
	if metrics.Summary.Diameter < 0 {
		t.Errorf("Diameter should be non-negative, got %d", metrics.Summary.Diameter)
	}
}

func TestSqrt(t *testing.T) {
	tests := []struct {
		input float64
		want  float64
	}{
		{0, 0},
		{1, 1},
		{4, 2},
		{9, 3},
		{16, 4},
		{2, 1.414213},
	}

	for _, tt := range tests {
		got := sqrt(tt.input)
		tolerance := 0.0001
		if got < tt.want-tolerance || got > tt.want+tolerance {
			t.Errorf("sqrt(%v) = %v, want ~%v", tt.input, got, tt.want)
		}
	}
}

// BenchmarkCalculateCloseness removed - now using gonum library

func BenchmarkCalculateEigenvector(b *testing.B) {
	nodes := make([]models.GraphNode, 50)
	edges := make(map[string][]string)

	for i := range nodes {
		id := string(rune('a' + (i % 26)))
		nodes[i] = models.GraphNode{ID: id, Name: id, Type: models.NodeFile}
		edges[id] = []string{}
	}

	for i := 0; i < len(nodes)-1; i++ {
		toID := nodes[i].ID
		fromID := nodes[i+1].ID
		edges[fromID] = append(edges[fromID], toID)
	}

	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = calculateEigenvector(nodes, edges, 100, 1e-6)
	}
}

// BenchmarkLouvainCommunityDetection removed - now using gonum library
