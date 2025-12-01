package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestJavaScript_LaunchDarkly(t *testing.T) {
	a, err := NewAnalyzer(
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
			code:     `const flag = ldClient.variation("my-feature", user, false);`,
			expected: []string{"my-feature"},
		},
		{
			name:     "variationDetail call",
			code:     `const detail = ldClient.variationDetail("another-flag", user, {});`,
			expected: []string{"another-flag"},
		},
		{
			name:     "boolVariation call",
			code:     `const isEnabled = ldClient.boolVariation("bool-flag", user, false);`,
			expected: []string{"bool-flag"},
		},
		{
			name:     "stringVariation call",
			code:     `const value = client.stringVariation("string-flag", user, "default");`,
			expected: []string{"string-flag"},
		},
		{
			name: "multiple flags",
			code: `
const flag1 = ldClient.variation("flag-one", user, false);
const flag2 = ldClient.variation("flag-two", user, true);
`,
			expected: []string{"flag-one", "flag-two"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.js")
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

func TestJavaScript_Split(t *testing.T) {
	a, err := NewAnalyzer(
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
			name:     "getTreatment call",
			code:     `const treatment = client.getTreatment(userId, "my-split");`,
			expected: []string{"my-split"},
		},
		{
			name:     "getTreatmentWithConfig call",
			code:     `const { treatment, config } = client.getTreatmentWithConfig(userId, "split-with-config");`,
			expected: []string{"split-with-config"},
		},
		{
			name:     "track call",
			code:     `client.track(userId, "split-event", { value: 100 });`,
			expected: []string{"split-event"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.js")
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

func TestJavaScript_Unleash(t *testing.T) {
	a, err := NewAnalyzer(
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
			name:     "isEnabled call",
			code:     `const enabled = unleash.isEnabled("my-toggle");`,
			expected: []string{"my-toggle"},
		},
		{
			name:     "getVariant call",
			code:     `const variant = unleash.getVariant("variant-toggle");`,
			expected: []string{"variant-toggle"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.js")
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

func TestJavaScript_PostHog(t *testing.T) {
	a, err := NewAnalyzer(
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
			name:     "isFeatureEnabled call",
			code:     `const enabled = posthog.isFeatureEnabled("posthog-flag");`,
			expected: []string{"posthog-flag"},
		},
		{
			name:     "getFeatureFlag call",
			code:     `const flag = posthog.getFeatureFlag("another-flag");`,
			expected: []string{"another-flag"},
		},
		{
			name:     "getFeatureFlagPayload call",
			code:     `const payload = posthog.getFeatureFlagPayload("payload-flag");`,
			expected: []string{"payload-flag"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.js")
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

func TestTypeScript_LaunchDarkly(t *testing.T) {
	a, err := NewAnalyzer(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.ts")
	code := `
import { LDClient } from 'launchdarkly-node-server-sdk';

interface User {
    key: string;
}

async function checkFlag(client: LDClient, user: User): Promise<boolean> {
    return await client.variation("typescript-flag", user, false);
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "typescript-flag", refs[0].FlagKey)
	assert.Equal(t, "launchdarkly", refs[0].Provider)
}

func TestTSX_LaunchDarkly(t *testing.T) {
	a, err := NewAnalyzer(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "Component.tsx")
	code := `
import React from 'react';
import { useLDClient } from 'launchdarkly-react-client-sdk';

export function FeatureComponent() {
    const ldClient = useLDClient();
    const isEnabled = ldClient.variation("react-feature", {}, false);

    return isEnabled ? <NewFeature /> : <OldFeature />;
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "react-feature", refs[0].FlagKey)
}

func TestJSX_LaunchDarkly(t *testing.T) {
	a, err := NewAnalyzer(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "Component.jsx")
	code := `
import React from 'react';
import { useLDClient } from 'launchdarkly-react-client-sdk';

export function FeatureComponent() {
    const ldClient = useLDClient();
    const isEnabled = ldClient.variation("jsx-feature", {}, false);

    return isEnabled ? <div>New</div> : <div>Old</div>;
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "jsx-feature", refs[0].FlagKey)
}
