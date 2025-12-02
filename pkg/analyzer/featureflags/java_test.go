package featureflags

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestJava_LaunchDarkly(t *testing.T) {
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
			name: "boolVariation call",
			code: `
public class FeatureService {
    public boolean checkFlag(LDUser user) {
        return ldClient.boolVariation("java-bool-flag", user, false);
    }
}
`,
			expected: []string{"java-bool-flag"},
		},
		{
			name: "stringVariation call",
			code: `
public class FeatureService {
    public String getValue(LDUser user) {
        return client.stringVariation("string-flag", user, "default");
    }
}
`,
			expected: []string{"string-flag"},
		},
		{
			name: "intVariation call",
			code: `
public class FeatureService {
    public int getCount(LDUser user) {
        return ldClient.intVariation("int-flag", user, 0);
    }
}
`,
			expected: []string{"int-flag"},
		},
		{
			name: "doubleVariation call",
			code: `
public class FeatureService {
    public double getRate(LDUser user) {
        return client.doubleVariation("double-flag", user, 0.0);
    }
}
`,
			expected: []string{"double-flag"},
		},
		{
			name: "jsonVariation call",
			code: `
public class FeatureService {
    public LDValue getConfig(LDUser user) {
        return ldClient.jsonValueVariation("json-flag", user, LDValue.buildObject().build());
    }
}
`,
			expected: []string{"json-flag"},
		},
		{
			name: "multiple flags",
			code: `
public class FeatureService {
    public void checkFlags(LDUser user) {
        boolean flag1 = ldClient.boolVariation("flag-one", user, false);
        boolean flag2 = ldClient.boolVariation("flag-two", user, false);
    }
}
`,
			expected: []string{"flag-one", "flag-two"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "Test.java")
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

func TestJava_PostHog(t *testing.T) {
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
			name: "isFeatureEnabled call",
			code: `
public class FeatureService {
    public boolean checkFlag(String userId) {
        return posthog.isFeatureEnabled("posthog-flag", userId);
    }
}
`,
			expected: []string{"posthog-flag"},
		},
		{
			name: "getFeatureFlag call",
			code: `
public class FeatureService {
    public Object getFlag(String userId) {
        return client.getFeatureFlag("get-flag", userId);
    }
}
`,
			expected: []string{"get-flag"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "Test.java")
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

func TestJava_NestedConditional(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "Test.java")
	code := `
public class FeatureService {
    public String processUser(User user) {
        if (user.isPremium()) {
            if (user.getRegion().equals("US")) {
                boolean flag = ldClient.boolVariation("nested-java-flag", user.ldUser(), false);
                if (flag) {
                    return "premium-us-feature";
                }
            }
        }
        return "default";
    }
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "nested-java-flag", refs[0].FlagKey)
	assert.Equal(t, 2, refs[0].NestingDepth)
}

func TestJava_SwitchStatement(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "Test.java")
	code := `
public class FeatureService {
    public String handleVariant(LDUser user) {
        String variant = ldClient.stringVariation("switch-flag", user, "default");
        switch (variant) {
            case "a":
                return "variant-a";
            case "b":
                return "variant-b";
            default:
                return "control";
        }
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

func TestJava_SpringAnnotation(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "Test.java")
	code := `
@Service
public class FeatureService {
    @Autowired
    private LDClient ldClient;

    public boolean isFeatureEnabled(LDUser user) {
        return ldClient.boolVariation("spring-flag", user, false);
    }
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "spring-flag", refs[0].FlagKey)
}

func TestJava_LambdaExpression(t *testing.T) {
	a, err := New(
		WithGitHistory(false),
		WithProviders([]string{"launchdarkly"}),
	)
	require.NoError(t, err)
	defer a.Close()

	dir := t.TempDir()
	path := filepath.Join(dir, "Test.java")
	code := `
public class FeatureService {
    public void processWithLambda(List<User> users) {
        users.stream()
            .filter(user -> ldClient.boolVariation("lambda-flag", user.ldUser(), false))
            .forEach(this::process);
    }
}
`
	err = os.WriteFile(path, []byte(code), 0644)
	require.NoError(t, err)

	refs, err := a.AnalyzeFile(path)
	require.NoError(t, err)
	require.Len(t, refs, 1)
	assert.Equal(t, "lambda-flag", refs[0].FlagKey)
}
