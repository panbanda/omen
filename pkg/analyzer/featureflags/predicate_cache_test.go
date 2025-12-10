package featureflags

import (
	"testing"

	sitter "github.com/smacker/go-tree-sitter"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/panbanda/omen/pkg/parser"
)

func TestQuerySet_CachesRegexPredicates(t *testing.T) {
	// Create a registry which loads queries with #match? predicates
	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	// Get a QuerySet that uses #match? predicates (e.g., Ruby Flipper)
	queries := registry.GetQueries(parser.LangRuby, []string{"flipper"})
	require.NotEmpty(t, queries, "flipper queries should exist for Ruby")

	qs := queries[0]

	// The QuerySet should have pre-compiled regexes
	assert.NotNil(t, qs.cachedRegexes, "QuerySet should have cached regexes")
	assert.NotEmpty(t, qs.cachedRegexes, "cachedRegexes should not be empty for queries with #match? predicates")
}

func TestQuerySet_FilterPredicates_MatchesBehavior(t *testing.T) {
	// This test verifies that FilterPredicates correctly applies predicates
	// to filter out non-matching patterns.
	//
	// NOTE: Tree-sitter's native cursor.FilterPredicates has a bug where it
	// doesn't properly apply #eq? predicates when patterns are wrapped with
	// outer parentheses for predicate association. Our implementation correctly
	// handles this case.

	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	// Get a QuerySet with predicates
	queries := registry.GetQueries(parser.LangRuby, []string{"flipper"})
	require.NotEmpty(t, queries)
	qs := queries[0]

	// Parse Ruby code with Flipper flags
	p := parser.New()
	defer p.Close()

	code := []byte(`
Flipper.enabled?(:my_feature_flag)
Flipper.disable(:another_flag)
SomeOtherClass.enabled?(:not_a_flag)
`)

	result, err := p.Parse(code, parser.LangRuby, "test.rb")
	require.NoError(t, err)
	require.NotNil(t, result.Tree)

	// Execute the query and collect matches with our FilterPredicates
	cursor := sitter.NewQueryCursor()
	defer cursor.Close()
	cursor.Exec(qs.Query, result.Tree.RootNode())

	var cachedMatches []*sitter.QueryMatch
	var flagKeys []string

	for {
		match, ok := cursor.NextMatch()
		if !ok {
			break
		}

		// Get result from our FilterPredicates implementation
		cachedResult := qs.FilterPredicates(match, code)
		if cachedResult != nil {
			cachedMatches = append(cachedMatches, cachedResult)
			// Extract flag key
			for _, cap := range cachedResult.Captures {
				if qs.Query.CaptureNameForId(cap.Index) == "flag_key" {
					flagKeys = append(flagKeys, cap.Node.Content(code))
				}
			}
		}
	}

	// Should match exactly 2 Flipper calls, NOT SomeOtherClass
	assert.Len(t, cachedMatches, 2,
		"should match exactly 2 Flipper calls (not SomeOtherClass)")

	assert.Contains(t, flagKeys, ":my_feature_flag")
	assert.Contains(t, flagKeys, ":another_flag")
	assert.NotContains(t, flagKeys, ":not_a_flag",
		"SomeOtherClass.enabled? should be filtered out by #eq? predicate")
}

func TestQuerySet_FilterPredicates_NoPredicates(t *testing.T) {
	// Test that queries without predicates still work
	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	// Add a simple query without predicates
	err = registry.AddQuery(parser.LangJavaScript, "no-predicates", `(call_expression) @call`)
	require.NoError(t, err)

	queries := registry.GetQueries(parser.LangJavaScript, []string{"no-predicates"})
	require.NotEmpty(t, queries)
	qs := queries[0]

	// Parse some JavaScript
	p := parser.New()
	defer p.Close()

	code := []byte(`foo(); bar();`)
	result, err := p.Parse(code, parser.LangJavaScript, "test.js")
	require.NoError(t, err)

	cursor := sitter.NewQueryCursor()
	defer cursor.Close()
	cursor.Exec(qs.Query, result.Tree.RootNode())

	matchCount := 0
	for {
		match, ok := cursor.NextMatch()
		if !ok {
			break
		}

		// FilterPredicates should return the match unchanged
		filtered := qs.FilterPredicates(match, code)
		if filtered != nil {
			matchCount++
		}
	}

	assert.Equal(t, 2, matchCount, "should match both function calls")
}

func TestQuerySet_FilterPredicates_EqPredicate(t *testing.T) {
	// Test #eq? predicate handling - cached version should match original behavior
	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	// Ruby Flipper uses #eq? for receiver matching
	queries := registry.GetQueries(parser.LangRuby, []string{"flipper"})
	require.NotEmpty(t, queries)
	qs := queries[0]

	p := parser.New()
	defer p.Close()

	testCases := []struct {
		name string
		code string
	}{
		{"Flipper match", `Flipper.enabled?(:my_flag)`},
		{"NotFlipper no match", `NotFlipper.enabled?(:some_flag)`},
		{"lowercase flipper", `flipper.enabled?(:other_flag)`},
	}

	for _, tc := range testCases {
		t.Run(tc.name, func(t *testing.T) {
			code := []byte(tc.code)
			result, err := p.Parse(code, parser.LangRuby, "test.rb")
			require.NoError(t, err)

			cursor := sitter.NewQueryCursor()
			defer cursor.Close()

			// Count with original FilterPredicates
			cursor.Exec(qs.Query, result.Tree.RootNode())
			var originalMatches []string
			for {
				match, ok := cursor.NextMatch()
				if !ok {
					break
				}
				filtered := cursor.FilterPredicates(match, code)
				if filtered != nil {
					for _, capture := range filtered.Captures {
						captureName := qs.Query.CaptureNameForId(capture.Index)
						if captureName == "flag_key" {
							originalMatches = append(originalMatches, capture.Node.Content(code))
							break
						}
					}
				}
			}

			// Count with cached FilterPredicates
			cursor.Exec(qs.Query, result.Tree.RootNode())
			var cachedMatches []string
			for {
				match, ok := cursor.NextMatch()
				if !ok {
					break
				}
				filtered := qs.FilterPredicates(match, code)
				if filtered != nil {
					for _, capture := range filtered.Captures {
						captureName := qs.Query.CaptureNameForId(capture.Index)
						if captureName == "flag_key" {
							cachedMatches = append(cachedMatches, capture.Node.Content(code))
							break
						}
					}
				}
			}

			// Cached version should produce identical results to original
			assert.Equal(t, originalMatches, cachedMatches,
				"cached should match original behavior for: %s", tc.code)
		})
	}
}
