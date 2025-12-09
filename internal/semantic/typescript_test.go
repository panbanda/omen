package semantic

import (
	"testing"

	"github.com/panbanda/omen/pkg/parser"
)

func TestTypeScriptExtractor_FunctionValues(t *testing.T) {
	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "function passed to array method",
			code: `function processItem(item: string) {
  return item.toUpperCase();
}

const items = ["a", "b", "c"];
const result = items.map(processItem);`,
			expected: []string{"processItem"},
		},
		{
			name: "function assigned to object property",
			code: `function handleClick() {}
function handleSubmit() {}

const handlers = {
  click: handleClick,
  submit: handleSubmit
};`,
			expected: []string{"handleClick", "handleSubmit"},
		},
		{
			name: "function passed to higher-order function",
			code: `function validator(value: string) {
  return value.length > 0;
}

function processWithValidation(fn: (v: string) => boolean) {}

processWithValidation(validator);`,
			expected: []string{"validator"},
		},
		{
			name: "function in array",
			code: `function step1() {}
function step2() {}
function step3() {}

const pipeline = [step1, step2, step3];`,
			expected: []string{"step1", "step2", "step3"},
		},
		{
			name: "builtins filtered out",
			code: `const data = JSON.parse('{}');
console.log(data);
const arr = Array.from([1, 2, 3]);`,
			expected: []string{},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangTypeScript, "test.ts")
			if err != nil {
				t.Fatalf("failed to parse: %v", err)
			}
			defer result.Tree.Close()

			extractor := newTypeScriptExtractor()
			defer extractor.Close()

			refs := extractor.ExtractRefs(result.Tree, result.Source)

			got := make(map[string]bool)
			for _, ref := range refs {
				if ref.Kind == RefFunctionValue {
					got[ref.Name] = true
				}
			}

			for _, want := range tt.expected {
				if !got[want] {
					t.Errorf("expected to find %q, but didn't", want)
				}
			}
		})
	}
}

func TestTypeScriptExtractor_Decorators(t *testing.T) {
	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "NestJS controller decorators",
			code: `import { Controller, Get, Post } from '@nestjs/common';

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
}`,
			expected: []string{"findAll", "create"},
		},
		{
			name: "method decorator",
			code: `function Log(target: any, key: string) {}

class Service {
  @Log
  processData() {
    return true;
  }
}`,
			expected: []string{"processData"},
		},
		{
			name: "multiple decorators on method",
			code: `class API {
  @Authenticated
  @RateLimit(100)
  @Cache(3600)
  getData() {
    return {};
  }
}`,
			expected: []string{"getData"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangTypeScript, "test.ts")
			if err != nil {
				t.Fatalf("failed to parse: %v", err)
			}
			defer result.Tree.Close()

			extractor := newTypeScriptExtractor()
			defer extractor.Close()

			refs := extractor.ExtractRefs(result.Tree, result.Source)

			got := make(map[string]bool)
			for _, ref := range refs {
				if ref.Kind == RefDecorator {
					got[ref.Name] = true
				}
			}

			for _, want := range tt.expected {
				if !got[want] {
					t.Errorf("expected to find %q with RefDecorator kind, but didn't", want)
				}
			}
		})
	}
}

func TestTypeScriptExtractor_DynamicCalls(t *testing.T) {
	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "bracket notation with string literal",
			code: `const obj = {
  process: () => {},
  handle: () => {}
};

obj["process"]();`,
			expected: []string{"process"},
		},
		{
			name: "string literal bracket access",
			code: `const api = {
  getData: () => {},
};

api["getData"]();`,
			expected: []string{"getData"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p := parser.New()
			defer p.Close()

			result, err := p.Parse([]byte(tt.code), parser.LangTypeScript, "test.ts")
			if err != nil {
				t.Fatalf("failed to parse: %v", err)
			}
			defer result.Tree.Close()

			extractor := newTypeScriptExtractor()
			defer extractor.Close()

			refs := extractor.ExtractRefs(result.Tree, result.Source)

			got := make(map[string]bool)
			for _, ref := range refs {
				if ref.Kind == RefDynamicCall {
					got[ref.Name] = true
				}
			}

			for _, want := range tt.expected {
				if !got[want] {
					t.Errorf("expected to find %q with RefDynamicCall kind, but didn't", want)
				}
			}
		})
	}
}

func TestTypeScriptExtractor_RefKinds(t *testing.T) {
	code := `function handler() {}

const routes = {
  home: handler
};

class Controller {
  @Get()
  index() {}
}

const obj = {};
obj["method"]();`

	p := parser.New()
	defer p.Close()

	result, err := p.Parse([]byte(code), parser.LangTypeScript, "test.ts")
	if err != nil {
		t.Fatalf("failed to parse: %v", err)
	}
	defer result.Tree.Close()

	extractor := newTypeScriptExtractor()
	defer extractor.Close()

	refs := extractor.ExtractRefs(result.Tree, result.Source)

	kindByName := make(map[string]RefKind)
	for _, ref := range refs {
		kindByName[ref.Name] = ref.Kind
	}

	if kind, ok := kindByName["handler"]; !ok || kind != RefFunctionValue {
		t.Errorf("expected 'handler' to be RefFunctionValue, got %v", kind)
	}

	if kind, ok := kindByName["index"]; !ok || kind != RefDecorator {
		t.Errorf("expected 'index' to be RefDecorator, got %v", kind)
	}

	if kind, ok := kindByName["method"]; !ok || kind != RefDynamicCall {
		t.Errorf("expected 'method' to be RefDynamicCall, got %v", kind)
	}
}

func TestTypeScriptExtractor_NilTree(t *testing.T) {
	extractor := newTypeScriptExtractor()
	defer extractor.Close()

	refs := extractor.ExtractRefs(nil, nil)
	if refs != nil {
		t.Errorf("expected nil for nil tree, got %v", refs)
	}
}

func TestTypeScriptExtractor_ForLanguageReturnsExtractor(t *testing.T) {
	langs := []parser.Language{
		parser.LangTypeScript,
		parser.LangJavaScript,
		parser.LangTSX,
	}

	for _, lang := range langs {
		extractor := ForLanguage(lang)
		if extractor == nil {
			t.Errorf("expected extractor for %v, got nil", lang)
		} else {
			extractor.Close()
		}
	}
}
