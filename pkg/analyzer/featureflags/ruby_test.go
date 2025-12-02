package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestRuby_Flipper(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"flipper"}),
	)
	require.NoError(t, err)
	defer a.Close()

	tests := []struct {
		name     string
		code     string
		expected []string
	}{
		{
			name: "enabled? call with symbol",
			code: `
class FeatureService
  def check_flag(user)
    Flipper.enabled?(:ruby_feature, user)
  end
end
`,
			expected: []string{"ruby_feature"},
		},
		{
			name: "enabled? call with string",
			code: `
def check_flag
  Flipper.enabled?("string-feature")
end
`,
			expected: []string{"string-feature"},
		},
		{
			name: "enable call",
			code: `
Flipper.enable(:new_feature)
`,
			expected: []string{"new_feature"},
		},
		{
			name: "disable call",
			code: `
Flipper.disable(:old_feature)
`,
			expected: []string{"old_feature"},
		},
		{
			name: "feature method call",
			code: `
feature = Flipper.feature(:my_feature)
feature.enable
`,
			expected: []string{"my_feature"},
		},
		{
			name: "multiple flags",
			code: `
class FeatureService
  def check_features(user)
    flag1 = Flipper.enabled?(:flag_one, user)
    flag2 = Flipper.enabled?(:flag_two, user)
    flag1 && flag2
  end
end
`,
			expected: []string{"flag_one", "flag_two"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.rb")
			err := os.WriteFile(path, []byte(tt.code), 0644)
			require.NoError(t, err)

			refs, err := a.AnalyzeFile(path)
			require.NoError(t, err)

			flagKeys := make([]string, len(refs))
			for i, ref := range refs {
				flagKeys[i] = ref.FlagKey
				assert.Equal(t, "flipper", ref.Provider)
			}
			assert.ElementsMatch(t, tt.expected, flagKeys)
		})
	}
}

func TestRuby_PostHog(t *testing.T) {
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
			name: "is_feature_enabled call",
			code: `
def check_flag(user_id)
  posthog.is_feature_enabled("posthog-flag", user_id)
end
`,
			expected: []string{"posthog-flag"},
		},
		{
			name: "feature_enabled? call",
			code: `
def check_flag(distinct_id)
  client.feature_enabled?("ruby-posthog", distinct_id)
end
`,
			expected: []string{"ruby-posthog"},
		},
		{
			name: "get_feature_flag call",
			code: `
def get_flag(user_id)
  PostHog.get_feature_flag("get-flag", user_id)
end
`,
			expected: []string{"get-flag"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "test.rb")
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

func TestRuby_NestedConditional(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"flipper"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.rb")
	code := `
class FeatureService
  def process_user(user)
    if user.premium?
      if user.region == "US"
        if Flipper.enabled?(:nested_ruby_flag, user)
          "premium-us-feature"
        end
      end
    end
  end
end
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "nested_ruby_flag", refs[0].FlagKey)
	// Ruby if statements nest
	assert.GreaterOrEqual(t, refs[0].NestingDepth, 2)
}

func TestRuby_UnlessStatement(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"flipper"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.rb")
	code := `
def check_feature(user)
  unless Flipper.enabled?(:unless_flag, user)
    fallback_behavior
  end
end
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "unless_flag", refs[0].FlagKey)
	// Unless is a conditional
	assert.GreaterOrEqual(t, refs[0].NestingDepth, 1)
}

func TestRuby_CaseStatement(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"flipper"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.rb")
	code := `
class FeatureService
  def handle_variant(user)
    variant = Flipper[:case_flag].variant(user)
    case variant
    when "a"
      "variant-a"
    when "b"
      "variant-b"
    else
      "control"
    end
  end
end
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	_, err = a.AnalyzeFile(path)
	require.NoError(t, err)
	// The [] accessor syntax may or may not be captured depending on query
	// Main test is that we don't crash on case statements
}

func TestRuby_RailsController(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"flipper"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.rb")
	code := `
class FeaturesController < ApplicationController
  before_action :authenticate_user!

  def index
    if Flipper.enabled?(:new_dashboard, current_user)
      render :new_dashboard
    else
      render :legacy_dashboard
    end
  end

  def show
    @feature_enabled = Flipper.enabled?(:show_feature, current_user)
  end
end
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 2)

	flagKeys := make([]string, len(refs))
	for i, ref := range refs {
		flagKeys[i] = ref.FlagKey
	}
	assert.ElementsMatch(t, []string{"new_dashboard", "show_feature"}, flagKeys)
}

func TestRuby_BlockSyntax(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"flipper"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.rb")
	code := `
users.each do |user|
  if Flipper.enabled?(:block_flag, user)
    process_user(user)
  end
end
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "block_flag", refs[0].FlagKey)
}
