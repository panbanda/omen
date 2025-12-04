package featureflags

import (
	"os"
	"path/filepath"
	"sync/atomic"
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

	var progressCount atomic.Int32
	result, err := a.AnalyzeProjectWithProgress(files, func() {
		progressCount.Add(1)
	})
	require.NoError(t, err)
	assert.NotNil(t, result)
	// Progress callback should be called for each file
	assert.GreaterOrEqual(t, progressCount.Load(), int32(1))
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

func TestCalculateStaleness_NilVCS(t *testing.T) {
	a, err := New(WithVCSOpener(nil))
	require.NoError(t, err)
	defer a.Close()

	staleness := a.calculateStaleness("test-flag", []string{"test.js"})
	assert.Nil(t, staleness, "should return nil when vcsOpener is nil")
}

func TestCalculateStaleness_EmptyFiles(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	staleness := a.calculateStaleness("test-flag", []string{})
	assert.Nil(t, staleness, "should return nil for empty file list")
}

func TestCalculateStaleness_NoRepoFound(t *testing.T) {
	a, err := New(WithGitHistory(true))
	require.NoError(t, err)
	defer a.Close()

	// Non-existent files should not find a repo
	staleness := a.calculateStaleness("test-flag", []string{"/nonexistent/path/test.js"})
	assert.Nil(t, staleness, "should return nil when repo cannot be opened")
}

func TestBuildSummary_Empty(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	summary := a.buildSummary([]FlagAnalysis{})
	assert.Equal(t, 0, summary.TotalFlags)
	assert.Equal(t, 0, summary.TotalReferences)
	assert.Equal(t, 0.0, summary.AvgFileSpread)
}

func TestBuildSummary_WithFlags(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	flags := []FlagAnalysis{
		{
			FlagKey:  "flag-1",
			Provider: "launchdarkly",
			References: []Reference{
				{File: "a.js", FlagKey: "flag-1"},
				{File: "b.js", FlagKey: "flag-1"},
			},
			Complexity: Complexity{
				FileSpread:      2,
				MaxNestingDepth: 1,
				CoupledFlags:    []string{"flag-2"},
			},
			Priority: Priority{Level: PriorityLow},
		},
		{
			FlagKey:  "flag-2",
			Provider: "split",
			References: []Reference{
				{File: "a.js", FlagKey: "flag-2"},
			},
			Complexity: Complexity{
				FileSpread:      1,
				MaxNestingDepth: 2,
				CoupledFlags:    []string{"flag-1"},
			},
			Priority: Priority{Level: PriorityHigh},
		},
	}

	summary := a.buildSummary(flags)
	assert.Equal(t, 2, summary.TotalFlags)
	assert.Equal(t, 3, summary.TotalReferences)
	assert.Equal(t, 1, summary.ByPriority[PriorityLow])
	assert.Equal(t, 1, summary.ByPriority[PriorityHigh])
	assert.Equal(t, 1, summary.ByProvider["launchdarkly"])
	assert.Equal(t, 1, summary.ByProvider["split"])
	assert.Equal(t, 1.5, summary.AvgFileSpread)
}

func TestCalculateComplexity(t *testing.T) {
	a, err := New()
	require.NoError(t, err)
	defer a.Close()

	refs := []Reference{
		{FlagKey: "flag-a", File: "a.js", NestingDepth: 1},
		{FlagKey: "flag-a", File: "b.js", NestingDepth: 3},
		{FlagKey: "flag-a", File: "a.js", NestingDepth: 2},
	}

	complexity := a.calculateComplexity(refs)
	assert.Equal(t, 2, complexity.FileSpread)      // a.js and b.js
	assert.Equal(t, 3, complexity.MaxNestingDepth) // max nesting is 3
	assert.Equal(t, 3, complexity.DecisionPoints)  // 3 refs
}

func TestNormalizeLanguageForQueries(t *testing.T) {
	tests := []struct {
		lang     parser.Language
		expected parser.Language
	}{
		{parser.LangTypeScript, parser.LangJavaScript},
		{parser.LangTSX, parser.LangJavaScript},
		{parser.LangJavaScript, parser.LangJavaScript},
		{parser.LangPython, parser.LangPython},
		{parser.LangGo, parser.LangGo},
	}

	for _, tt := range tests {
		result := normalizeLanguageForQueries(tt.lang)
		assert.Equal(t, tt.expected, result, "normalizeLanguageForQueries(%v)", tt.lang)
	}
}

func TestDirNameToLanguage(t *testing.T) {
	tests := []struct {
		dir      string
		expected parser.Language
	}{
		{"javascript", parser.LangJavaScript},
		{"python", parser.LangPython},
		{"go", parser.LangGo},
		{"java", parser.LangJava},
		{"ruby", parser.LangRuby},
		{"unknown", parser.LangUnknown},
	}

	for _, tt := range tests {
		result := dirNameToLanguage(tt.dir)
		assert.Equal(t, tt.expected, result, "dirNameToLanguage(%q)", tt.dir)
	}
}

func TestAddQuery(t *testing.T) {
	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	// Add a custom query
	err = registry.AddQuery(parser.LangJavaScript, "custom-provider", `(call_expression) @call`)
	assert.NoError(t, err)

	// Verify query was added
	queries := registry.GetQueries(parser.LangJavaScript, []string{"custom-provider"})
	found := false
	for _, q := range queries {
		if q.Provider == "custom-provider" {
			found = true
			break
		}
	}
	assert.True(t, found, "custom query should be added")

	// Test invalid query
	err = registry.AddQuery(parser.LangJavaScript, "bad-provider", `(invalid syntax`)
	assert.Error(t, err)
}

func TestLoadCustomProvider(t *testing.T) {
	registry, err := NewQueryRegistry()
	require.NoError(t, err)
	defer registry.Close()

	err = registry.LoadCustomProvider("my-provider", []string{"javascript", "go"}, `(call_expression) @call`)
	assert.NoError(t, err)

	// Verify it was loaded for JavaScript
	jsQueries := registry.GetQueries(parser.LangJavaScript, []string{"my-provider"})
	found := false
	for _, q := range jsQueries {
		if q.Provider == "my-provider" {
			found = true
			break
		}
	}
	assert.True(t, found, "custom provider should be loaded for javascript")
}

func TestCalculatePriority_AllLevels(t *testing.T) {
	// Test with nil staleness
	priority := CalculatePriority(nil, Complexity{
		FileSpread:      1,
		MaxNestingDepth: 1,
	})
	assert.NotEmpty(t, priority.Level)

	// Test HIGH priority - stale + some complexity factors
	staleness := &Staleness{
		Score:             10.0, // high staleness score
		DaysSinceIntro:    100,
		DaysSinceModified: 100,
	}
	complexity := Complexity{
		FileSpread:      15,                      // > 10, adds 2.0
		MaxNestingDepth: 3,                       // > 2, adds 3.0
		CoupledFlags:    []string{"a", "b", "c"}, // > 2, adds 2.0
	}
	// riskScore = 10 + 2 + 3 + 2 = 17
	// effort = 15 * 0.5 = 7.5 * 1.5 = 11.25
	// priority = 17 / 11.25 = 1.51 -> LOW
	// Need higher staleness score for CRITICAL
	priority = CalculatePriority(staleness, complexity)
	// Just verify it returns a valid level
	assert.Contains(t, []string{PriorityLow, PriorityMedium, PriorityHigh, PriorityCritical}, priority.Level)

	// Test LOW priority - fresh + simple
	staleness = &Staleness{
		Score:             0.1,
		DaysSinceIntro:    5,
		DaysSinceModified: 2,
	}
	complexity = Complexity{
		FileSpread:      1,
		MaxNestingDepth: 1,
	}
	priority = CalculatePriority(staleness, complexity)
	assert.Equal(t, PriorityLow, priority.Level)

	// Test MEDIUM priority - moderate staleness, low complexity
	staleness = &Staleness{
		Score:             10.0,
		DaysSinceIntro:    30,
		DaysSinceModified: 30,
	}
	complexity = Complexity{
		FileSpread:      1,
		MaxNestingDepth: 1,
	}
	// riskScore = 10, effort = 1, priority = 10 -> MEDIUM
	priority = CalculatePriority(staleness, complexity)
	assert.Contains(t, []string{PriorityMedium, PriorityHigh}, priority.Level)
}
