package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestPython_LaunchDarkly(t *testing.T) {
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
			name:     "variation call",
			code:     `flag_value = ld_client.variation("python-flag", user, False)`,
			expected: []string{"python-flag"},
		},
		{
			name:     "variation_detail call",
			code:     `detail = ld_client.variation_detail("detail-flag", user, {})`,
			expected: []string{"detail-flag"},
		},
		{
			name:     "bool_variation call",
			code:     `is_enabled = client.bool_variation("bool-flag", user, False)`,
			expected: []string{"bool-flag"},
		},
		{
			name:     "string_variation call",
			code:     `value = client.string_variation("string-flag", user, "default")`,
			expected: []string{"string-flag"},
		},
		{
			name:     "int_variation call",
			code:     `count = client.int_variation("int-flag", user, 0)`,
			expected: []string{"int-flag"},
		},
		{
			name: "multiple flags",
			code: `
flag1 = ld_client.variation("flag-one", user, False)
flag2 = ld_client.variation("flag-two", user, True)
`,
			expected: []string{"flag-one", "flag-two"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.py")
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

func TestPython_Split(t *testing.T) {
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
			name:     "get_treatment call",
			code:     `treatment = client.get_treatment(user_id, "split-flag")`,
			expected: []string{"split-flag"},
		},
		{
			name:     "get_treatment_with_config call",
			code:     `treatment, config = client.get_treatment_with_config(user_id, "config-flag")`,
			expected: []string{"config-flag"},
		},
		{
			name:     "get_treatments call",
			code:     `treatments = client.get_treatments(user_id, ["flag1", "flag2"])`,
			expected: []string{}, // Array not captured as single flag
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.py")
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

func TestPython_PostHog(t *testing.T) {
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
			name:     "feature_enabled call",
			code:     `enabled = posthog.feature_enabled("posthog-flag", user_id)`,
			expected: []string{"posthog-flag"},
		},
		{
			name:     "get_feature_flag call",
			code:     `flag = posthog.get_feature_flag("get-flag", user_id)`,
			expected: []string{"get-flag"},
		},
		{
			name:     "get_feature_flag_payload call",
			code:     `payload = posthog.get_feature_flag_payload("payload-flag", user_id)`,
			expected: []string{"payload-flag"},
		},
		{
			name:     "is_feature_enabled call",
			code:     `is_on = client.is_feature_enabled("enabled-flag", distinct_id)`,
			expected: []string{"enabled-flag"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.py")
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

func TestPython_NestedConditional(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.py")
	code := `
def process_user(user):
    if user.is_premium:
        if user.region == "US":
            flag = ld_client.variation("nested-flag", user, False)
            if flag:
                return "premium-us-feature"
    return "default"
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "nested-flag", refs[0].FlagKey)
	assert.Equal(t, 2, refs[0].NestingDepth)
}

func TestPython_ClassMethod(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.py")
	code := `
class FeatureService:
    def __init__(self, ld_client):
        self.ld_client = ld_client

    def is_feature_on(self, user):
        return self.ld_client.variation("class-method-flag", user, False)
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "class-method-flag", refs[0].FlagKey)
}
