package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestGo_LaunchDarkly(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "BoolVariation call",
			code: `
package main

func check() bool {
	flag, _ := ldClient.BoolVariation("go-bool-flag", user, false)
	return flag
}
`,
			expected: []string{"go-bool-flag"},
		},
		{
			name: "StringVariation call",
			code: `
package main

func getValue() string {
	value, _ := client.StringVariation("string-flag", ctx, "default")
	return value
}
`,
			expected: []string{"string-flag"},
		},
		{
			name: "IntVariation call",
			code: `
package main

func getCount() int {
	count, _ := ldClient.IntVariation("int-flag", user, 0)
	return count
}
`,
			expected: []string{"int-flag"},
		},
		{
			name: "Float64Variation call",
			code: `
package main

func getRate() float64 {
	rate, _ := client.Float64Variation("float-flag", ctx, 0.0)
	return rate
}
`,
			expected: []string{"float-flag"},
		},
		{
			name: "JSONVariation call",
			code: `
package main

func getConfig() interface{} {
	var config map[string]interface{}
	client.JSONVariation("json-flag", user, nil, &config)
	return config
}
`,
			expected: []string{"json-flag"},
		},
		{
			name: "AllFlags call",
			code: `
package main

func getAllFlags() {
	flags := ldClient.AllFlagsState(user)
	_ = flags
}
`,
			expected: []string{}, // AllFlagsState doesn't capture individual flag
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.go")
			err := os.WriteFile(path, []byte(tt.code), 0644)
			require.NoError(t, err)

			refs, err := a.AnalyzeFile(path)
			require.NoError(t, err)

			flagKeys := make([]string, len(refs))
			for i, ref := range refs {
				flagKeys[i] = ref.FlagKey
				assert.Equal(t, "launchdarkly", ref.Provider)
			}
			assert.ElementsMatch(t, tt.expected, flagKeys)
		})
	}
}

func TestGo_Split(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"split"}),
	)
	require.NoError(t, err)
	defer a.Close()

	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "Treatment call",
			code: `
package main

func getTreatment() string {
	treatment := client.Treatment(userID, "split-flag", nil)
	return treatment
}
`,
			expected: []string{"split-flag"},
		},
		{
			name: "TreatmentWithConfig call",
			code: `
package main

func getTreatmentConfig() (string, map[string]interface{}) {
	treatment, config := client.TreatmentWithConfig(userID, "config-flag", nil)
	return treatment, config
}
`,
			expected: []string{"config-flag"},
		},
		{
			name: "Track call",
			code: `
package main

func trackEvent() {
	client.Track(userID, "track-event", nil, nil)
}
`,
			expected: []string{"track-event"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.go")
			err := os.WriteFile(path, []byte(tt.code), 0644)
			require.NoError(t, err)

			refs, err := a.AnalyzeFile(path)
			require.NoError(t, err)

			flagKeys := make([]string, len(refs))
			for i, ref := range refs {
				flagKeys[i] = ref.FlagKey
				assert.Equal(t, "split", ref.Provider)
			}
			assert.ElementsMatch(t, tt.expected, flagKeys)
		})
	}
}

func TestGo_Unleash(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"unleash"}),
	)
	require.NoError(t, err)
	defer a.Close()

	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "IsEnabled call",
			code: `
package main

func isFeatureOn() bool {
	return unleash.IsEnabled("unleash-toggle")
}
`,
			expected: []string{"unleash-toggle"},
		},
		{
			name: "GetVariant call",
			code: `
package main

func getVariant() *unleash.Variant {
	return unleash.GetVariant("variant-toggle")
}
`,
			expected: []string{"variant-toggle"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.go")
			err := os.WriteFile(path, []byte(tt.code), 0644)
			require.NoError(t, err)

			refs, err := a.AnalyzeFile(path)
			require.NoError(t, err)

			flagKeys := make([]string, len(refs))
			for i, ref := range refs {
				flagKeys[i] = ref.FlagKey
				assert.Equal(t, "unleash", ref.Provider)
			}
			assert.ElementsMatch(t, tt.expected, flagKeys)
		})
	}
}

func TestGo_PostHog(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"posthog"}),
	)
	require.NoError(t, err)
	defer a.Close()

	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "IsFeatureEnabled call",
			code: `
package main

func isOn() bool {
	return client.IsFeatureEnabled("posthog-flag", "user-123", nil)
}
`,
			expected: []string{"posthog-flag"},
		},
		{
			name: "GetFeatureFlag call",
			code: `
package main

func getFlag() interface{} {
	return posthog.GetFeatureFlag("get-flag", distinctID, nil)
}
`,
			expected: []string{"get-flag"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.go")
			err := os.WriteFile(path, []byte(tt.code), 0644)
			require.NoError(t, err)

			refs, err := a.AnalyzeFile(path)
			require.NoError(t, err)

			flagKeys := make([]string, len(refs))
			for i, ref := range refs {
				flagKeys[i] = ref.FlagKey
				assert.Equal(t, "posthog", ref.Provider)
			}
			assert.ElementsMatch(t, tt.expected, flagKeys)
		})
	}
}

func TestGo_NestedConditional(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	code := `
package main

func processRequest(user User) string {
	if user.IsPremium {
		if user.Region == "US" {
			flag, _ := ldClient.BoolVariation("nested-go-flag", user.Context, false)
			if flag {
				return "premium-us-feature"
			}
		}
	}
	return "default"
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "nested-go-flag", refs[0].FlagKey)
	assert.Equal(t, 2, refs[0].NestingDepth)
}

func TestGo_SwitchStatement(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	code := `
package main

func handleVariant(user User) string {
	variant, _ := ldClient.StringVariation("switch-flag", user.Context, "default")
	switch variant {
	case "a":
		return "variant-a"
	case "b":
		return "variant-b"
	default:
		return "control"
	}
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "switch-flag", refs[0].FlagKey)
}

func TestGo_MethodReceiver(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.go")
	code := `
package main

type FeatureService struct {
	ldClient *ld.LDClient
}

func (s *FeatureService) IsEnabled(user User, flagKey string) bool {
	flag, _ := s.ldClient.BoolVariation("method-receiver-flag", user.Context, false)
	return flag
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "method-receiver-flag", refs[0].FlagKey)
}
