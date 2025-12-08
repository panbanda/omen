package semantic

import (
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestGoExtractor_FunctionValues(t *testing.T) {
	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "short variable declaration",
			code: `package main

func process() {}

func main() {
	handler := process
	_ = handler
}`,
			expected: []string{"process"},
		},
		{
			name: "function in map literal with string key",
			code: `package main

func handleGet() {}

var routes = map[string]func(){
	"get": handleGet,
}`,
			expected: []string{"handleGet"},
		},
		{
			name: "function in struct literal",
			code: `package main

func onStart() {}
func onStop() {}

type Config struct {
	Start func()
	Stop  func()
}

var cfg = Config{
	Start: onStart,
	Stop:  onStop,
}`,
			expected: []string{"onStart", "onStop"},
		},
		{
			name: "function passed to goroutine",
			code: `package main

func worker() {}

func main() {
	go worker()
}`,
			expected: []string{},
		},
		{
			name: "function in slice",
			code: `package main

func step1() {}
func step2() {}

var pipeline = []func(){step1, step2}`,
			expected: []string{"step1", "step2"},
		},
		{
			name: "builtin functions filtered out",
			code: `package main

func main() {
	data := make([]int, 10)
	length := len(data)
	_ = length
}`,
			expected: []string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangGo, "test.go")
			if err != nil {
				t.Fatalf("failed to parse: %v", err)
			}
			defer result.Tree.Close()

			extractor := newGoExtractor()
			defer extractor.Close()

			refs := extractor.ExtractRefs(result.Tree, result.Source)

			got := make(map[string]bool)
			for _, ref := range refs {
				got[ref.Name] = true
				if ref.Kind != RefFunctionValue {
					t.Errorf("expected RefFunctionValue, got %v for %s", ref.Kind, ref.Name)
				}
			}

			for _, want := range tt.expected {
				if !got[want] {
					t.Errorf("expected to find %q, but didn't", want)
				}
			}

			if len(refs) != len(tt.expected) {
				var names []string
				for _, ref := range refs {
					names = append(names, ref.Name)
				}
				t.Errorf("expected %d refs, got %d: %v", len(tt.expected), len(refs), names)
			}
		})
	}
}

func TestGoExtractor_NilTree(t *testing.T) {
	extractor := newGoExtractor()
	defer extractor.Close()

	refs := extractor.ExtractRefs(nil, nil)
	if refs != nil {
		t.Errorf("expected nil for nil tree, got %v", refs)
	}
}

func TestGoExtractor_RefKindString(t *testing.T) {
	if RefFunctionValue.String() != "function_value" {
		t.Errorf("expected 'function_value', got %q", RefFunctionValue.String())
	}
}
