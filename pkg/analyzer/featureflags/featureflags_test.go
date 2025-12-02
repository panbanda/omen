package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/panbanda/omen/pkg/parser"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestNew(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	assert.NotNil(t, a.parser)
	assert.NotNil(t, a.registry)
}

func TestNewWithOptions(t *testing.T) {
	a, err := New(
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
	a, err := New(WithGitHistory(false))
	require.NoError(t, err)
	defer a.Close()

	result, err := a.AnalyzeProject([]string{})
	require.NoError(t, err)
	assert.NotNil(t, result)
	assert.Empty(t, result.Flags)
	assert.Equal(t, 0, result.Summary.TotalFlags)
}

func TestAnalyzeNonexistentFile(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	refs, err := a.AnalyzeFile("/nonexistent/file.js")
	assert.Error(t, err)
	assert.Nil(t, refs)
}

func TestAnalyzeUnsupportedLanguage(t *testing.T) {
	a, err := New()
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
	a, err := New(WithMaxFileSize(10)) // 10 bytes limit
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
		staleness  *Staleness
		complexity Complexity
	}{
		{
			name:      "no staleness data - should not panic",
			staleness: nil,
			complexity: Complexity{
				FileSpread:     1,
				DecisionPoints: 1,
			},
		},
		{
			name: "high staleness + high complexity",
			staleness: &Staleness{
				Score: 50.0, // Very high staleness score
			},
			complexity: Complexity{
				FileSpread:      15,
				MaxNestingDepth: 5,
				DecisionPoints:  20,
				CoupledFlags:    []string{"a", "b", "c"},
			},
		},
		{
			name: "low staleness + low complexity",
			staleness: &Staleness{
				Score: 0.5,
			},
			complexity: Complexity{
				FileSpread:     1,
				DecisionPoints: 1,
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			priority := CalculatePriority(tt.staleness, tt.complexity)
			// Just verify it doesn't panic and returns a valid level
			assert.Contains(t, []string{
				PriorityLow,
				PriorityMedium,
				PriorityHigh,
				PriorityCritical,
			}, priority.Level)
		})
	}
}

func TestProviderFiltering(t *testing.T) {
	a, err := New(
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
	a, err := New(WithGitHistory(false))
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
	a, err := New(WithGitHistory(false))
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
	a, err := New(
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
	var nestedRef *Reference
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

func TestStalenessScoreCalculation(t *testing.T) {
	tests := []struct {
		name            string
		daysSinceIntro  int
		daysSinceModify int
		expectedTTL     int
		expectPositive  bool
	}{
		{
			name:            "not stale - within TTL",
			daysSinceIntro:  7,
			daysSinceModify: 7,
			expectedTTL:     14,
			expectPositive:  false,
		},
		{
			name:            "stale - overdue past TTL",
			daysSinceIntro:  60,
			daysSinceModify: 60,
			expectedTTL:     14,
			expectPositive:  true,
		},
		{
			name:            "stale - no modifications",
			daysSinceIntro:  30,
			daysSinceModify: 90,
			expectedTTL:     14,
			expectPositive:  true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			staleness := &Staleness{
				DaysSinceIntro:    tt.daysSinceIntro,
				DaysSinceModified: tt.daysSinceModify,
			}
			staleness.CalculateStalenessScore(tt.expectedTTL)

			if tt.expectPositive {
				assert.Greater(t, staleness.Score, 0.0)
			} else {
				assert.Equal(t, 0.0, staleness.Score)
			}
		})
	}
}

func TestNewSummary(t *testing.T) {
	s := NewSummary()

	assert.NotNil(t, s.ByPriority)
	assert.NotNil(t, s.ByProvider)
	assert.NotNil(t, s.TopCoupled)
	assert.Equal(t, 0, s.ByPriority[PriorityLow])
	assert.Equal(t, 0, s.ByPriority[PriorityMedium])
	assert.Equal(t, 0, s.ByPriority[PriorityHigh])
	assert.Equal(t, 0, s.ByPriority[PriorityCritical])
}

func TestDefaultThresholds(t *testing.T) {
	defaults := DefaultThresholds()

	assert.Equal(t, 14, defaults.ExpectedTTLRelease)
	assert.Equal(t, 90, defaults.ExpectedTTLExperiment)
	assert.Equal(t, 4, defaults.FileSpreadWarning)
	assert.Equal(t, 10, defaults.FileSpreadCritical)
	assert.Equal(t, 2, defaults.NestingWarning)
	assert.Equal(t, 3, defaults.NestingCritical)
}

func TestFlagAnalysis_IsHighRisk(t *testing.T) {
	thresholds := DefaultThresholds()

	tests := []struct {
		name     string
		analysis FlagAnalysis
		expected bool
	}{
		{
			name: "low risk",
			analysis: FlagAnalysis{
				Complexity: Complexity{
					FileSpread:      1,
					MaxNestingDepth: 1,
				},
				Priority: Priority{Level: PriorityLow},
			},
			expected: false,
		},
		{
			name: "high file spread",
			analysis: FlagAnalysis{
				Complexity: Complexity{
					FileSpread:      15,
					MaxNestingDepth: 1,
				},
				Priority: Priority{Level: PriorityLow},
			},
			expected: true,
		},
		{
			name: "high nesting",
			analysis: FlagAnalysis{
				Complexity: Complexity{
					FileSpread:      1,
					MaxNestingDepth: 5,
				},
				Priority: Priority{Level: PriorityLow},
			},
			expected: true,
		},
		{
			name: "critical priority",
			analysis: FlagAnalysis{
				Complexity: Complexity{
					FileSpread:      1,
					MaxNestingDepth: 1,
				},
				Priority: Priority{Level: PriorityCritical},
			},
			expected: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := tt.analysis.IsHighRisk(thresholds)
			assert.Equal(t, tt.expected, result)
		})
	}
}

func TestFlagAnalysis_FilesAffected(t *testing.T) {
	analysis := FlagAnalysis{
		References: []Reference{
			{File: "a.js", FlagKey: "flag"},
			{File: "b.js", FlagKey: "flag"},
			{File: "a.js", FlagKey: "flag"}, // duplicate
			{File: "c.js", FlagKey: "flag"},
		},
	}

	files := analysis.FilesAffected()

	assert.Len(t, files, 3)
}

func TestWithVCSOpener(t *testing.T) {
	// Just verify the option works without error
	a, err := New(WithVCSOpener(nil))
	require.NoError(t, err)
	defer a.Close()

	assert.Nil(t, a.vcsOpener)
}

func TestWithCustomProviders(t *testing.T) {
	custom := CustomProvider{
		Name:      "test-provider",
		Languages: []string{"javascript"},
		Query:     `(call_expression function: (identifier) @fn (#eq? @fn "testFlag") arguments: (arguments (string) @flag_key))`,
	}

	a, err := New(WithCustomProviders([]CustomProvider{custom}))
	require.NoError(t, err)
	defer a.Close()

	assert.Contains(t, a.customProviders, custom)
}

func TestAnalyzer_GetSupportedLanguages(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	languages := a.GetSupportedLanguages()
	assert.NotEmpty(t, languages)
}

func TestAnalyzer_GetProviders(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	providers := a.GetProviders(parser.LangJavaScript)
	assert.NotEmpty(t, providers)
	assert.Contains(t, providers, "launchdarkly")
}
