package featureflags

import (
	"testing"

	"github.com/panbanda/omen/pkg/parser"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// TestNestingDepth_AllLanguages verifies nesting depth calculation works for all supported languages
func TestNestingDepth_AllLanguages(t *testing.T) {
	a, err := New(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	p := parser.New()
	defer p.Close()

	tests := []struct {
		name     string
		lang     parser.Language
		code     string
		line     uint32
		expected int
	}{
		// Go
		{
			name:     "Go if_statement",
			lang:     parser.LangGo,
			code:     "package main\nfunc f() {\n  if true {\n    x := 1\n  }\n}",
			line:     4,
			expected: 1,
		},
		{
			name:     "Go expression_switch_statement",
			lang:     parser.LangGo,
			code:     "package main\nfunc f() {\n  switch x {\n  case 1:\n    y := 2\n  }\n}",
			line:     5,
			expected: 2, // switch + case
		},
		{
			name:     "Go nested if in switch",
			lang:     parser.LangGo,
			code:     "package main\nfunc f() {\n  switch x {\n  case 1:\n    if true {\n      y := 2\n    }\n  }\n}",
			line:     6,
			expected: 3, // switch + case + if
		},
		// Python
		{
			name:     "Python if_statement",
			lang:     parser.LangPython,
			code:     "if True:\n    x = 1",
			line:     2,
			expected: 1,
		},
		{
			name:     "Python elif_clause",
			lang:     parser.LangPython,
			code:     "if False:\n    pass\nelif True:\n    x = 1",
			line:     4,
			expected: 1, // elif is part of if
		},
		{
			name:     "Python match_statement",
			lang:     parser.LangPython,
			code:     "match x:\n    case 1:\n        y = 2",
			line:     3,
			expected: 2, // match + case
		},
		{
			name:     "Python conditional_expression",
			lang:     parser.LangPython,
			code:     "if True:\n    x = 1 if y else 2",
			line:     2,
			expected: 2, // if_statement + conditional_expression
		},
		// JavaScript
		{
			name:     "JavaScript if_statement",
			lang:     parser.LangJavaScript,
			code:     "if (true) {\n  let x = 1;\n}",
			line:     2,
			expected: 1,
		},
		{
			name:     "JavaScript switch_case",
			lang:     parser.LangJavaScript,
			code:     "switch (x) {\n  case 1:\n    let y = 2;\n}",
			line:     3,
			expected: 2, // switch + case
		},
		{
			name:     "JavaScript ternary_expression",
			lang:     parser.LangJavaScript,
			code:     "if (true) {\n  let x = y ? 1 : 2;\n}",
			line:     2,
			expected: 2, // if + ternary
		},
		// Java
		{
			name:     "Java if_statement",
			lang:     parser.LangJava,
			code:     "class T { void f() {\n  if (true) {\n    int x = 1;\n  }\n}}",
			line:     3,
			expected: 1,
		},
		{
			name:     "Java switch_expression",
			lang:     parser.LangJava,
			code:     "class T { void f() {\n  switch (x) {\n    case 1:\n      int y = 2;\n  }\n}}",
			line:     4,
			expected: 2, // switch + case/label
		},
		{
			name:     "Java ternary_expression",
			lang:     parser.LangJava,
			code:     "class T { void f() {\n  if (true) {\n    int x = y ? 1 : 2;\n  }\n}}",
			line:     3,
			expected: 2, // if + ternary
		},
		// Ruby
		{
			name:     "Ruby if",
			lang:     parser.LangRuby,
			code:     "if true\n  x = 1\nend",
			line:     2,
			expected: 1,
		},
		{
			name:     "Ruby unless",
			lang:     parser.LangRuby,
			code:     "unless false\n  x = 1\nend",
			line:     2,
			expected: 1,
		},
		{
			name:     "Ruby case/when",
			lang:     parser.LangRuby,
			code:     "case x\nwhen 1\n  y = 2\nend",
			line:     3,
			expected: 2, // case + when
		},
		{
			name:     "Ruby elsif",
			lang:     parser.LangRuby,
			code:     "if false\n  x = 0\nelsif true\n  x = 1\nend",
			line:     4,
			expected: 1, // elsif is part of if
		},
		{
			name:     "Ruby conditional (ternary)",
			lang:     parser.LangRuby,
			code:     "if true\n  x = y ? 1 : 2\nend",
			line:     2,
			expected: 2, // if + conditional
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result, err := p.Parse([]byte(tt.code), tt.lang, "test.txt")
			require.NoError(t, err)
			require.NotNil(t, result.Tree)

			depth := a.calculateNestingDepth(result, tt.line)
			assert.Equal(t, tt.expected, depth,
				"wrong nesting depth for %s at line %d\ncode:\n%s",
				tt.name, tt.line, tt.code)
		})
	}
}

func TestCalculateNestingDepthBatch(t *testing.T) {
	a, err := New(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	p := parser.New()
	defer p.Close()

	code := []byte(`
if Flipper.enabled?(:flag1)
  puts "level 1"
  if Flipper.enabled?(:flag2)
    puts "level 2"
    if Flipper.enabled?(:flag3)
      puts "level 3"
    end
  end
end
Flipper.enabled?(:flag0)
`)

	result, err := p.Parse(code, parser.LangRuby, "test.rb")
	require.NoError(t, err)
	require.NotNil(t, result.Tree)

	// Lines where flags appear (1-indexed):
	// flag1: line 2 (inside 0 conditionals when the check happens, depth 0)
	// flag2: line 4 (inside 1 conditional - the if from line 2)
	// flag3: line 6 (inside 2 conditionals - ifs from line 2 and 4)
	// flag0: line 11 (outside all conditionals)

	lines := []uint32{2, 4, 6, 11}

	// Calculate using batch method
	batchDepths := a.calculateNestingDepthBatch(result, lines)

	// Calculate using individual method for comparison
	individualDepths := make(map[uint32]int)
	for _, line := range lines {
		individualDepths[line] = a.calculateNestingDepth(result, line)
	}

	// Both methods should produce the same results
	for _, line := range lines {
		assert.Equal(t, individualDepths[line], batchDepths[line],
			"depth mismatch for line %d: individual=%d, batch=%d",
			line, individualDepths[line], batchDepths[line])
	}
}

func TestCalculateNestingDepthBatch_EmptyLines(t *testing.T) {
	a, err := New(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	p := parser.New()
	defer p.Close()

	code := []byte(`puts "hello"`)

	result, err := p.Parse(code, parser.LangRuby, "test.rb")
	require.NoError(t, err)

	// Empty lines slice should return empty map
	depths := a.calculateNestingDepthBatch(result, []uint32{})
	assert.Empty(t, depths)
}

func TestCalculateNestingDepthBatch_JavaScript(t *testing.T) {
	a, err := New(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	p := parser.New()
	defer p.Close()

	code := []byte(`
if (ldClient.variation("flag1", user, false)) {
  console.log("level 1");
  if (ldClient.variation("flag2", user, false)) {
    console.log("level 2");
  }
}
const x = ldClient.variation("flag0", user, false);
`)

	result, err := p.Parse(code, parser.LangJavaScript, "test.js")
	require.NoError(t, err)
	require.NotNil(t, result.Tree)

	lines := []uint32{2, 4, 8}

	batchDepths := a.calculateNestingDepthBatch(result, lines)
	individualDepths := make(map[uint32]int)
	for _, line := range lines {
		individualDepths[line] = a.calculateNestingDepth(result, line)
	}

	for _, line := range lines {
		assert.Equal(t, individualDepths[line], batchDepths[line],
			"depth mismatch for line %d", line)
	}
}
