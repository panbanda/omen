package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/panbanda/omen/pkg/config"
)

// TestGetPaths verifies path handling from CLI arguments.
func TestGetPaths(t *testing.T) {
	tests := []struct {
		name     string
		args     []string
		expected []string
	}{
		{
			name:     "no args defaults to current dir",
			args:     []string{},
			expected: []string{"."},
		},
		{
			name:     "single path",
			args:     []string{"/foo/bar"},
			expected: []string{"/foo/bar"},
		},
		{
			name:     "multiple paths",
			args:     []string{"/foo", "/bar"},
			expected: []string{"/foo", "/bar"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := getPaths(tt.args)
			if len(result) != len(tt.expected) {
				t.Errorf("getPaths() = %v, want %v", result, tt.expected)
				return
			}
			for i := range result {
				if result[i] != tt.expected[i] {
					t.Errorf("getPaths()[%d] = %q, want %q", i, result[i], tt.expected[i])
				}
			}
		})
	}
}

// TestValidateDays verifies the days validation function.
func TestValidateDays(t *testing.T) {
	tests := []struct {
		days    int
		wantErr bool
	}{
		{days: 1, wantErr: false},
		{days: 30, wantErr: false},
		{days: 365, wantErr: false},
		{days: 0, wantErr: true},
		{days: -1, wantErr: true},
		{days: -100, wantErr: true},
	}

	for _, tt := range tests {
		err := validateDays(tt.days)
		if (err != nil) != tt.wantErr {
			t.Errorf("validateDays(%d) error = %v, wantErr %v", tt.days, err, tt.wantErr)
		}
	}
}

// TestTruncate verifies the truncate helper function.
func TestTruncate(t *testing.T) {
	tests := []struct {
		input    string
		maxLen   int
		expected string
	}{
		{input: "hello", maxLen: 10, expected: "hello"},
		{input: "hello world", maxLen: 8, expected: "hello..."},
		{input: "", maxLen: 5, expected: ""},
		{input: "hi", maxLen: 2, expected: "hi"},
	}

	for _, tt := range tests {
		result := truncate(tt.input, tt.maxLen)
		if result != tt.expected {
			t.Errorf("truncate(%q, %d) = %q, want %q", tt.input, tt.maxLen, result, tt.expected)
		}
	}
}

// TestSanitizeID verifies the sanitizeID helper function.
func TestSanitizeID(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{input: "simple", expected: "simple"},
		{input: "with-dash", expected: "with_dash"},
		{input: "with/slash", expected: "with_slash"},
		{input: "with.dot", expected: "with_dot"},
		{input: "with space", expected: "with_space"},
	}

	for _, tt := range tests {
		result := sanitizeID(tt.input)
		if result != tt.expected {
			t.Errorf("sanitizeID(%q) = %q, want %q", tt.input, result, tt.expected)
		}
	}
}

// TestVersionVariable verifies version variables are defined.
func TestVersionVariable(t *testing.T) {
	if version == "" {
		t.Error("version variable should have a default value")
	}
}

// TestComplexityCommandE2E tests the complexity command end-to-end.
func TestComplexityCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

func simple() {
	x := 1
	_ = x
}

func complex() {
	for i := 0; i < 10; i++ {
		if i%2 == 0 {
			continue
		}
	}
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	// Reset root command for testing
	rootCmd.SetArgs([]string{"analyze", "complexity", "-f", "json", tmpDir})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("complexity command failed: %v", err)
	}
}

// TestSATDCommandE2E tests the SATD command end-to-end.
func TestSATDCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

// TODO: fix this important bug
func buggy() {
	// HACK: temporary workaround
	x := 1
	_ = x
}

// FIXME: needs refactoring
func broken() {}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	rootCmd.SetArgs([]string{"analyze", "satd", "-f", "json", tmpDir})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("satd command failed: %v", err)
	}
}

// TestDeadcodeCommandE2E tests the deadcode command end-to-end.
func TestDeadcodeCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

func used() {}

func unused() {}

func main() {
	used()
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	rootCmd.SetArgs([]string{"analyze", "deadcode", "-f", "json", tmpDir})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("deadcode command failed: %v", err)
	}
}

// TestTDGCommandE2E tests the TDG command end-to-end.
func TestTDGCommandE2E(t *testing.T) {
	tmpDir := t.TempDir()
	goFile := filepath.Join(tmpDir, "test.go")
	content := `package main

// TODO: needs refactoring
func complex() {
	for i := 0; i < 10; i++ {
		if i%2 == 0 {
			if i%4 == 0 {
				continue
			}
		}
	}
}
`
	if err := os.WriteFile(goFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write test file: %v", err)
	}

	rootCmd.SetArgs([]string{"analyze", "tdg", "-f", "json", tmpDir})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("tdg command failed: %v", err)
	}
}

// TestNoFilesError verifies commands handle empty directories gracefully.
func TestNoFilesError(t *testing.T) {
	tmpDir := t.TempDir()

	rootCmd.SetArgs([]string{"analyze", "complexity", tmpDir})
	// Should not crash, may return error for no files
	_ = rootCmd.Execute()
}

// TestInitCommand verifies the init command creates a config file.
func TestInitCommand(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.toml")

	rootCmd.SetArgs([]string{"init", "-o", configPath})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("init command failed: %v", err)
	}

	// Verify file was created
	if _, err := os.Stat(configPath); os.IsNotExist(err) {
		t.Fatal("init command did not create config file")
	}

	// Verify file content is valid TOML
	content, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatalf("failed to read config file: %v", err)
	}

	if len(content) == 0 {
		t.Fatal("config file is empty")
	}

	// Verify it contains expected sections
	contentStr := string(content)
	for _, section := range []string{"[analysis]", "[thresholds]", "[cache]", "[output]"} {
		if !strings.Contains(contentStr, section) {
			t.Errorf("config file missing section %q", section)
		}
	}
}

// TestInitCommandRefusesOverwrite verifies init refuses to overwrite without --force.
func TestInitCommandRefusesOverwrite(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.toml")

	// Create existing file
	if err := os.WriteFile(configPath, []byte("existing"), 0644); err != nil {
		t.Fatalf("failed to create test file: %v", err)
	}

	rootCmd.SetArgs([]string{"init", "-o", configPath})
	err := rootCmd.Execute()
	if err == nil {
		t.Fatal("init command should have failed when file exists")
	}

	// Verify original content preserved
	content, _ := os.ReadFile(configPath)
	if string(content) != "existing" {
		t.Error("init command overwrote file without --force")
	}
}

// TestInitCommandForce verifies init --force overwrites existing files.
func TestInitCommandForce(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.toml")

	// Create existing file
	if err := os.WriteFile(configPath, []byte("existing"), 0644); err != nil {
		t.Fatalf("failed to create test file: %v", err)
	}

	rootCmd.SetArgs([]string{"init", "-o", configPath, "--force"})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("init --force command failed: %v", err)
	}

	// Verify file was overwritten
	content, _ := os.ReadFile(configPath)
	if string(content) == "existing" {
		t.Error("init --force did not overwrite file")
	}
}

// TestInitCommandCreatesDirectory verifies init creates parent directories.
func TestInitCommandCreatesDirectory(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "subdir", "nested", "omen.toml")

	rootCmd.SetArgs([]string{"init", "-o", configPath})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("init command failed: %v", err)
	}

	if _, err := os.Stat(configPath); os.IsNotExist(err) {
		t.Fatal("init command did not create config file in nested directory")
	}
}

// TestConfigValidateCommand verifies config validate works on valid config.
func TestConfigValidateCommand(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.toml")

	// Create a valid config file
	content := `[analysis]
churn_days = 30

[thresholds]
cyclomatic_complexity = 10
cognitive_complexity = 15
duplicate_min_lines = 6
duplicate_similarity = 0.8
dead_code_confidence = 0.8
defect_high_risk = 0.6
tdg_high_risk = 2.5

[duplicates]
min_tokens = 50
similarity_threshold = 0.70
shingle_size = 5
num_hash_functions = 200
num_bands = 20
rows_per_band = 10
min_group_size = 2

[cache]
ttl = 24
`
	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write config: %v", err)
	}

	rootCmd.SetArgs([]string{"config", "validate", "-c", configPath})
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("config validate failed on valid config: %v", err)
	}
}

// TestConfigValidateInvalid verifies config validate catches invalid values.
func TestConfigValidateInvalid(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "bad.toml")

	// Create an invalid config file
	content := `[thresholds]
cyclomatic_complexity = 0
duplicate_similarity = 1.5
`
	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write config: %v", err)
	}

	rootCmd.SetArgs([]string{"config", "validate", "-c", configPath})
	err := rootCmd.Execute()
	if err == nil {
		t.Fatal("config validate should have failed on invalid config")
	}
}

// TestConfigValidateMissingFile verifies config validate handles missing files.
func TestConfigValidateMissingFile(t *testing.T) {
	rootCmd.SetArgs([]string{"config", "validate", "-c", "/nonexistent/path/omen.toml"})
	err := rootCmd.Execute()
	if err == nil {
		t.Fatal("config validate should have failed for missing file")
	}
}

// TestConfigShowCommand verifies config show outputs configuration.
func TestConfigShowCommand(t *testing.T) {
	rootCmd.SetArgs([]string{"config", "show"})
	// Should not error when showing defaults
	err := rootCmd.Execute()
	if err != nil {
		t.Fatalf("config show failed: %v", err)
	}
}

// TestGenerateDefaultConfig verifies the generated config is valid.
func TestGenerateDefaultConfig(t *testing.T) {
	content, err := generateDefaultConfig()
	if err != nil {
		t.Fatalf("generateDefaultConfig failed: %v", err)
	}

	if len(content) == 0 {
		t.Fatal("generateDefaultConfig returned empty string")
	}

	// Verify it contains expected sections
	for _, section := range []string{"[analysis]", "[thresholds]", "[duplicates]", "[exclude]", "[cache]", "[output]", "[feature_flags]"} {
		if !strings.Contains(content, section) {
			t.Errorf("generated config missing section %q", section)
		}
	}
}

// TestFindConfigFile verifies config file discovery.
func TestFindConfigFile(t *testing.T) {
	tmpDir := t.TempDir()
	originalWd, _ := os.Getwd()
	defer os.Chdir(originalWd)

	os.Chdir(tmpDir)

	// No config file should return empty
	result := config.FindConfigFile()
	if result != "" {
		t.Errorf("FindConfigFile() = %q, want empty string", result)
	}

	// Create omen.toml
	if err := os.WriteFile("omen.toml", []byte("# config"), 0644); err != nil {
		t.Fatalf("failed to write config: %v", err)
	}

	result = config.FindConfigFile()
	if result != "omen.toml" {
		t.Errorf("FindConfigFile() = %q, want %q", result, "omen.toml")
	}
}
