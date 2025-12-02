package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestCustomProvider_Feature(t *testing.T) {
	query := `
; Feature.enabled?(:flag_name, context) - custom provider
(call
  receiver: (constant) @receiver
  (#eq? @receiver "Feature")
  method: (identifier) @method
  (#match? @method "^(enabled\\?|get_feature_flag)$")
  arguments: (argument_list
    .
    (simple_symbol) @flag_key))
`

	customProviders := []CustomProvider{
		{
			Name:      "feature",
			Languages: []string{"ruby"},
			Query:     query,
		},
	}

	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"feature"}),
		WithCustomProviders(customProviders),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "test.rb")
	code := `
class FeatureService
  def check_flag(user)
    Feature.enabled?(:my_custom_flag, order)
  end

  def get_variant(user)
    Feature.get_feature_flag(:variant_flag, user)
  end
end
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)

	t.Logf("Found %d references", len(refs))
	for _, ref := range refs {
		t.Logf("  %s (%s) at line %d", ref.FlagKey, ref.Provider, ref.Line)
	}

	require.Len(t, refs, 2)

	flagKeys := make([]string, len(refs))
	for i, ref := range refs {
		flagKeys[i] = ref.FlagKey
		assert.Equal(t, "feature", ref.Provider)
	}
	assert.ElementsMatch(t, []string{"my_custom_flag", "variant_flag"}, flagKeys)
}
