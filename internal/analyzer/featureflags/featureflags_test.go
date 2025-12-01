package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestNewAnalyzer(t *testing.T) {
	a, err := NewAnalyzer()
	require.NoError(t, err)
	defer a.Close()

	assert.NotNil(t, a.parser)
	assert.NotNil(t, a.registry)
}

func TestNewAnalyzerWithOptions(t *testing.T) {
	a, err := NewAnalyzer(
		WithProviders([]string{"launchdarkly"}),
		WithMaxFileSize(1024*1024),
		WithGitHistory(false),
		WithExpectedTTL(30),
	)
	require.NoError(t, err)
	defer a.Close()

	assert.Equal(t, []string{"launchdarkly"}, a.providers)
	assert.Equal(t, int64(1024*1024), a.maxFileSize)
	assert.False(t, a.includeGit)
	assert.Equal(t, 30, a.expectedTTL)
}

func TestQueryRegistry(t *testing.T) {
	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	// Check supported languages
	languages := registry.GetAllLanguages()
	assert.NotEmpty(t, languages)

	// Check providers for JavaScript
	providers := registry.GetProviders(parser.LangJavaScript)
	assert.Contains(t, providers, "launchdarkly")
	assert.Contains(t, providers, "split")
	assert.Contains(t, providers, "unleash")
	assert.Contains(t, providers, "posthog")
}

func TestLanguageToDirName(t *testing.T) {
	tests := []struct {
		lang     parser.Language
		expected string
	}{
		{parser.LangJavaScript, "javascript"},
		{parser.LangTypeScript, "javascript"},
		{parser.LangTSX, "javascript"},
		{parser.LangPython, "python"},
		{parser.LangGo, "go"},
		{parser.LangJava, "java"},
		{parser.LangRuby, "ruby"},
		{parser.LangUnknown, ""},
	}

	for _, tt := range tests {
		t.Run(string(tt.lang), func(t *testing.T) {
			result := LanguageToDirName(tt.lang)
			assert.Equal(t, tt.expected, result)
		})
	}
}

func TestAnalyzeEmptyProject(t *testing.T) {
	a, err := NewAnalyzer(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	result, err := a.AnalyzeProject([]string{})
	require.NoError(t, err)
	assert.NotNil(t, result)
	assert.Empty(t, result.Flags)
	assert.Equal(t, 0, result.Summary.TotalFlags)
}

func TestAnalyzeNonexistentFile(t *testing.T) {
	a, err := NewAnalyzer()
	require.NoError(t, err)
	defer a.Close()

	refs, err := a.AnalyzeFile("/nonexistent/file.js")
	assert.Error(t, err)
	assert.Nil(t, refs)
}

func TestAnalyzeUnsupportedLanguage(t *testing.T) {
	a, err := NewAnalyzer()
	require.NoError(t, err)
	defer a.Close()

	// Create temp file with unsupported extension
	dir := t.TempDir()
	path := filepath.Join(dir, "test.xyz")
	err = os.WriteFile(path, []byte("some content"), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	// Unsupported language returns an error
	assert.Error(t, err)
	assert.Nil(t, refs)
}

func TestFileSizeLimit(t *testing.T) {
	a, err := NewAnalyzer(WithMaxFileSize(10)) // 10 bytes limit
	require.NoError(t, err)
	defer a.Close()

	// Create temp file larger than limit
	dir := t.TempDir()
	path := filepath.Join(dir, "test.js")
	content := `const flag = ldClient.variation("my-flag", user, false);`
	err = os.WriteFile(path, []byte(content), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	// File too large returns an error
	assert.Error(t, err)
	assert.Nil(t, refs)
}

func TestPriorityCalculation(t *testing.T) {
	tests := []struct {
		name       string
		staleness  *models.FlagStaleness
		complexity models.FlagComplexity
	}{
		{
			name:      "no staleness data - should not panic",
			staleness: nil,
			complexity: models.FlagComplexity{
				FileSpread:     1,
				DecisionPoints: 1,
			},
		},
		{
			name: "high staleness + high complexity",
			staleness: &models.FlagStaleness{
				Score: 50.0, // Very high staleness score
			},
			complexity: models.FlagComplexity{
				FileSpread:      15,
				MaxNestingDepth: 5,
				DecisionPoints:  20,
				CoupledFlags:    []string{"a", "b", "c"},
			},
		},
		{
			name: "low staleness + low complexity",
			staleness: &models.FlagStaleness{
				Score: 0.5,
			},
			complexity: models.FlagComplexity{
				FileSpread:     1,
				DecisionPoints: 1,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			priority := models.CalculatePriority(tt.staleness, tt.complexity)
			// Just verify it doesn't panic and returns a valid level
			assert.Contains(t, []string{
				models.PriorityLow,
				models.PriorityMedium,
				models.PriorityHigh,
				models.PriorityCritical,
			}, priority.Level)
		})
	}
}

func TestProviderFiltering(t *testing.T) {
	a, err := NewAnalyzer(
		WithProviders([]string{"launchdarkly"}),
		WithGitHistory(false),
	)
	require.NoError(t, err)
	defer a.Close()

	// Create temp file with multiple provider flags
	dir := t.TempDir()
	path := filepath.Join(dir, "test.js")
	content := `
const ld = require('launchdarkly-node-server-sdk');
const split = require('@splitsoftware/splitio');

// LaunchDarkly flag
const ldFlag = ldClient.variation("ld-flag", user, false);

// Split flag - should not be detected with provider filter
const splitFlag = client.getTreatment(user, "split-flag");
`
	err = os.WriteFile(path, []byte(content), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)

	// All refs should be LaunchDarkly only
	for _, ref := range refs {
		assert.Equal(t, "launchdarkly", ref.Provider)
	}
	// At least one flag found
	assert.NotEmpty(t, refs)
}

func TestProgressCallback(t *testing.T) {
	a, err := NewAnalyzer(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	// Create temp files
	dir := t.TempDir()
	for i := 0; i < 3; i++ {
		path := filepath.Join(dir, "test_"+string(rune('a'+i))+".js")
		content := `const flag = ldClient.variation("flag-` + string(rune('a'+i)) + `", user, false);`
		err = os.WriteFile(path, []byte(content), 0644)
		require.NoError(t, err)
	}

	files := []string{
		filepath.Join(dir, "test_a.js"),
		filepath.Join(dir, "test_b.js"),
		filepath.Join(dir, "test_c.js"),
	}

	progressCount := 0
	result, err := a.AnalyzeProjectWithProgress(files, func() {
		progressCount++
	})
	require.NoError(t, err)
	assert.NotNil(t, result)
	// Progress callback should be called for each file
	assert.GreaterOrEqual(t, progressCount, 1)
}

func TestSummaryAggregation(t *testing.T) {
	a, err := NewAnalyzer(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	// Create temp file with multiple flags
	dir := t.TempDir()
	path := filepath.Join(dir, "test.js")
	content := `
const flag1 = ldClient.variation("flag-1", user, false);
const flag2 = ldClient.variation("flag-2", user, false);
const flag3 = client.getTreatment(user, "flag-3");
`
	err = os.WriteFile(path, []byte(content), 0644)
	require.NoError(t, err)

	result, err := a.AnalyzeProject([]string{path})
	require.NoError(t, err)

	assert.Equal(t, 3, result.Summary.TotalFlags)
	assert.GreaterOrEqual(t, result.Summary.TotalReferences, 3)
}

func TestNestingDepthCalculation(t *testing.T) {
	a, err := NewAnalyzer(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	// Create temp file with nested conditionals
	dir := t.TempDir()
	path := filepath.Join(dir, "test.js")
	content := `
function test() {
    if (condition1) {
        if (condition2) {
            const flag = ldClient.variation("nested-flag", user, false);
        }
    }
}
`
	err = os.WriteFile(path, []byte(content), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	// Find a ref with nesting depth > 0
	var nestedRef *models.FlagReference
	for i := range refs {
		if refs[i].NestingDepth >= 2 {
			nestedRef = &refs[i]
			break
		}
	}
	require.NotNil(t, nestedRef, "should find a nested flag")
	assert.Equal(t, "nested-flag", nestedRef.FlagKey)
	assert.Equal(t, 2, nestedRef.NestingDepth)
}
