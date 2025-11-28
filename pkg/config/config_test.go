package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestDefaultConfig(t *testing.T) {
	cfg := DefaultConfig()

	if cfg == nil {
		t.Fatal("DefaultConfig() returned nil")
	}

	// Check analysis defaults
	if !cfg.Analysis.Complexity {
		t.Error("Analysis.Complexity should be true by default")
	}
	if !cfg.Analysis.SATD {
		t.Error("Analysis.SATD should be true by default")
	}
	if !cfg.Analysis.DeadCode {
		t.Error("Analysis.DeadCode should be true by default")
	}
	if cfg.Analysis.ChurnDays != 30 {
		t.Errorf("Analysis.ChurnDays = %d, want 30", cfg.Analysis.ChurnDays)
	}

	// Check threshold defaults
	if cfg.Thresholds.CyclomaticComplexity != 10 {
		t.Errorf("Thresholds.CyclomaticComplexity = %d, want 10", cfg.Thresholds.CyclomaticComplexity)
	}
	if cfg.Thresholds.CognitiveComplexity != 15 {
		t.Errorf("Thresholds.CognitiveComplexity = %d, want 15", cfg.Thresholds.CognitiveComplexity)
	}
	if cfg.Thresholds.DuplicateSimilarity != 0.8 {
		t.Errorf("Thresholds.DuplicateSimilarity = %f, want 0.8", cfg.Thresholds.DuplicateSimilarity)
	}

	// Check exclude defaults
	if !cfg.Exclude.Gitignore {
		t.Error("Exclude.Gitignore should be true by default")
	}
	if len(cfg.Exclude.Dirs) == 0 {
		t.Error("Exclude.Dirs should have default values")
	}

	// Check cache defaults
	if !cfg.Cache.Enabled {
		t.Error("Cache.Enabled should be true by default")
	}
	if cfg.Cache.TTL != 24 {
		t.Errorf("Cache.TTL = %d, want 24", cfg.Cache.TTL)
	}

	// Check output defaults
	if cfg.Output.Format != "text" {
		t.Errorf("Output.Format = %s, want text", cfg.Output.Format)
	}
	if !cfg.Output.Color {
		t.Error("Output.Color should be true by default")
	}
}

func TestLoadTOML(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.toml")

	content := `
[analysis]
complexity = true
satd = false
churn_days = 30

[thresholds]
cyclomatic_complexity = 15

[exclude]
dirs = ["vendor", "custom_exclude"]
patterns = ["*_generated.go"]

[cache]
enabled = false

[output]
format = "json"
`

	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	cfg, err := Load(configPath)
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}

	if cfg.Analysis.SATD {
		t.Error("Analysis.SATD should be false")
	}
	if cfg.Analysis.ChurnDays != 30 {
		t.Errorf("Analysis.ChurnDays = %d, want 30", cfg.Analysis.ChurnDays)
	}
	if cfg.Thresholds.CyclomaticComplexity != 15 {
		t.Errorf("Thresholds.CyclomaticComplexity = %d, want 15", cfg.Thresholds.CyclomaticComplexity)
	}
	if cfg.Cache.Enabled {
		t.Error("Cache.Enabled should be false")
	}
	if cfg.Output.Format != "json" {
		t.Errorf("Output.Format = %s, want json", cfg.Output.Format)
	}
}

func TestLoadYAML(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.yaml")

	content := `
analysis:
  complexity: true
  satd: false
  churn_days: 45

thresholds:
  cyclomatic_complexity: 20

output:
  format: markdown
`

	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	cfg, err := Load(configPath)
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}

	if cfg.Analysis.SATD {
		t.Error("Analysis.SATD should be false")
	}
	if cfg.Analysis.ChurnDays != 45 {
		t.Errorf("Analysis.ChurnDays = %d, want 45", cfg.Analysis.ChurnDays)
	}
	if cfg.Thresholds.CyclomaticComplexity != 20 {
		t.Errorf("Thresholds.CyclomaticComplexity = %d, want 20", cfg.Thresholds.CyclomaticComplexity)
	}
	if cfg.Output.Format != "markdown" {
		t.Errorf("Output.Format = %s, want markdown", cfg.Output.Format)
	}
}

func TestLoadJSON(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.json")

	content := `{
  "analysis": {
    "complexity": true,
    "satd": false,
    "churn_days": 60
  },
  "thresholds": {
    "cyclomatic_complexity": 25
  },
  "output": {
    "format": "json"
  }
}`

	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	cfg, err := Load(configPath)
	if err != nil {
		t.Fatalf("Load() error: %v", err)
	}

	if cfg.Analysis.SATD {
		t.Error("Analysis.SATD should be false")
	}
	if cfg.Analysis.ChurnDays != 60 {
		t.Errorf("Analysis.ChurnDays = %d, want 60", cfg.Analysis.ChurnDays)
	}
	if cfg.Thresholds.CyclomaticComplexity != 25 {
		t.Errorf("Thresholds.CyclomaticComplexity = %d, want 25", cfg.Thresholds.CyclomaticComplexity)
	}
}

func TestLoadNonExistentFile(t *testing.T) {
	_, err := Load("/nonexistent/path/omen.toml")
	if err == nil {
		t.Error("Load() should return error for non-existent file")
	}
}

func TestLoadInvalidFile(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.toml")

	// Invalid TOML
	content := `[analysis
invalid toml`

	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	_, err := Load(configPath)
	if err == nil {
		t.Error("Load() should return error for invalid config")
	}
}

func TestLoadOrDefault(t *testing.T) {
	// In a directory without config files, should return defaults
	tmpDir := t.TempDir()
	oldWd, _ := os.Getwd()
	defer os.Chdir(oldWd)

	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("Failed to change directory: %v", err)
	}

	cfg := LoadOrDefault()
	if cfg == nil {
		t.Fatal("LoadOrDefault() returned nil")
	}

	// Should have default values
	if cfg.Analysis.ChurnDays != 30 {
		t.Errorf("LoadOrDefault() returned non-default ChurnDays: %d", cfg.Analysis.ChurnDays)
	}
}

func TestLoadOrDefaultWithConfigFile(t *testing.T) {
	tmpDir := t.TempDir()
	oldWd, _ := os.Getwd()
	defer os.Chdir(oldWd)

	// Create config file
	content := `
[analysis]
churn_days = 999
`
	if err := os.WriteFile(filepath.Join(tmpDir, "omen.toml"), []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	if err := os.Chdir(tmpDir); err != nil {
		t.Fatalf("Failed to change directory: %v", err)
	}

	cfg := LoadOrDefault()
	if cfg.Analysis.ChurnDays != 999 {
		t.Errorf("LoadOrDefault() should load from file, got ChurnDays=%d", cfg.Analysis.ChurnDays)
	}
}

func TestShouldExclude(t *testing.T) {
	cfg := DefaultConfig()

	tests := []struct {
		path string
		want bool
	}{
		// Excluded directories
		{"vendor/pkg/file.go", true},
		{"node_modules/pkg/file.js", true},
		{".git/objects/file", true},

		// Excluded patterns
		{"main_test.go", true},
		{"util_test.py", true},
		{"app.min.js", true},

		// Excluded extensions
		{"go.sum", true},
		{"package.lock", true},

		// Not excluded
		{"main.go", false},
		{"pkg/util/helper.go", false},
		{"app.js", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := cfg.ShouldExclude(tt.path)
			if got != tt.want {
				t.Errorf("ShouldExclude(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestShouldExcludeCustomPatterns(t *testing.T) {
	cfg := DefaultConfig()
	cfg.Exclude.Patterns = append(cfg.Exclude.Patterns, "*_generated.go", "*.pb.go")
	cfg.Exclude.Dirs = append(cfg.Exclude.Dirs, "custom_exclude")

	tests := []struct {
		path string
		want bool
	}{
		{"model_generated.go", true},
		{"service.pb.go", true},
		{"custom_exclude/file.go", true},
		{"main.go", false},
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := cfg.ShouldExclude(tt.path)
			if got != tt.want {
				t.Errorf("ShouldExclude(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestShouldExcludePathsWithSeparators(t *testing.T) {
	cfg := DefaultConfig()

	// Test paths with directory separators
	tests := []struct {
		path string
		want bool
	}{
		{filepath.Join("src", "vendor", "pkg", "file.go"), true},
		{filepath.Join("vendor", "file.go"), true},
		{filepath.Join("src", "main.go"), false},
		{filepath.Join("pkg", "vendor_utils.go"), false}, // "vendor" in name, not directory
	}

	for _, tt := range tests {
		t.Run(tt.path, func(t *testing.T) {
			got := cfg.ShouldExclude(tt.path)
			if got != tt.want {
				t.Errorf("ShouldExclude(%q) = %v, want %v", tt.path, got, tt.want)
			}
		})
	}
}

func TestExcludeConfigDefaults(t *testing.T) {
	cfg := DefaultConfig()

	// Check default excluded directories
	expectedDirs := []string{"vendor", "node_modules", ".git", "dist", "build"}
	for _, dir := range expectedDirs {
		found := false
		for _, d := range cfg.Exclude.Dirs {
			if d == dir {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Default Exclude.Dirs should contain %q", dir)
		}
	}

	// Check default excluded patterns
	if len(cfg.Exclude.Patterns) == 0 {
		t.Error("Default Exclude.Patterns should not be empty")
	}

	// Check default excluded extensions
	if len(cfg.Exclude.Extensions) == 0 {
		t.Error("Default Exclude.Extensions should not be empty")
	}
}
