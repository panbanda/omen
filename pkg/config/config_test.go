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
	if len(cfg.Exclude.Patterns) == 0 {
		t.Error("Exclude.Patterns should have default values")
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
patterns = ["vendor/", "custom_exclude/", "*_generated.go"]

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

	cfg, err := LoadOrDefault()
	if err != nil {
		t.Fatalf("LoadOrDefault() returned error: %v", err)
	}
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

	cfg, err := LoadOrDefault()
	if err != nil {
		t.Fatalf("LoadOrDefault() returned error: %v", err)
	}
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
		// Simple basename patterns (what ShouldExclude still handles)
		{"main_test.go", true},
		{"util_test.py", true},
		{"app.min.js", true},
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
	cfg.Exclude.Patterns = append(cfg.Exclude.Patterns, "*_generated.go", "*.pb.go", "custom_exclude/")

	tests := []struct {
		path string
		want bool
	}{
		{"model_generated.go", true},
		{"service.pb.go", true},
		// Directory exclusion is now handled by the scanner's gitignore matching
		// ShouldExclude only does simple basename matching for backward compatibility
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

	// ShouldExclude only matches basename patterns now
	// Directory-based exclusion is handled by the scanner's gitignore matching
	tests := []struct {
		path string
		want bool
	}{
		{filepath.Join("src", "main_test.go"), true},     // basename matches *_test.go
		{filepath.Join("src", "main.go"), false},         // no pattern match
		{filepath.Join("pkg", "vendor_utils.go"), false}, // "vendor" in name, not a pattern
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

	// Check default excluded patterns (using gitignore-style syntax)
	expectedPatterns := []string{"vendor/", "node_modules/", ".git/", "dist/", "build/", "*_test.go"}
	for _, pattern := range expectedPatterns {
		found := false
		for _, p := range cfg.Exclude.Patterns {
			if p == pattern {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("Default Exclude.Patterns should contain %q", pattern)
		}
	}

	// Check default excluded patterns exist
	if len(cfg.Exclude.Patterns) == 0 {
		t.Error("Default Exclude.Patterns should not be empty")
	}
}

func TestValidate(t *testing.T) {
	tests := []struct {
		name      string
		modify    func(*Config)
		wantError bool
		errorMsg  string
	}{
		{
			name:      "default config is valid",
			modify:    func(c *Config) {},
			wantError: false,
		},
		{
			name:      "negative churn days",
			modify:    func(c *Config) { c.Analysis.ChurnDays = 0 },
			wantError: true,
			errorMsg:  "churn_days must be at least 1",
		},
		{
			name:      "churn days too large",
			modify:    func(c *Config) { c.Analysis.ChurnDays = 4000 },
			wantError: true,
			errorMsg:  "churn_days must be at most 3650",
		},
		{
			name:      "cyclomatic complexity zero",
			modify:    func(c *Config) { c.Thresholds.CyclomaticComplexity = 0 },
			wantError: true,
			errorMsg:  "cyclomatic_complexity must be at least 1",
		},
		{
			name:      "similarity threshold out of range",
			modify:    func(c *Config) { c.Thresholds.DuplicateSimilarity = 1.5 },
			wantError: true,
			errorMsg:  "duplicate_similarity must be between 0 and 1",
		},
		{
			name:      "negative dead code confidence",
			modify:    func(c *Config) { c.Thresholds.DeadCodeConfidence = -0.1 },
			wantError: true,
			errorMsg:  "dead_code_confidence must be between 0 and 1",
		},
		{
			name:      "min tokens zero",
			modify:    func(c *Config) { c.Duplicates.MinTokens = 0 },
			wantError: true,
			errorMsg:  "min_tokens must be at least 1",
		},
		{
			name:      "min group size too small",
			modify:    func(c *Config) { c.Duplicates.MinGroupSize = 1 },
			wantError: true,
			errorMsg:  "min_group_size must be at least 2",
		},
		{
			name: "hash functions relationship mismatch",
			modify: func(c *Config) {
				c.Duplicates.NumHashFunctions = 100
				c.Duplicates.NumBands = 10
				c.Duplicates.RowsPerBand = 5 // 10 * 5 = 50 != 100
			},
			wantError: true,
			errorMsg:  "num_hash_functions",
		},
		{
			name:      "negative cache TTL",
			modify:    func(c *Config) { c.Cache.TTL = -1 },
			wantError: true,
			errorMsg:  "cache.ttl must be non-negative",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := DefaultConfig()
			tt.modify(cfg)

			err := cfg.Validate()

			if tt.wantError {
				if err == nil {
					t.Errorf("Validate() expected error containing %q, got nil", tt.errorMsg)
				} else if tt.errorMsg != "" && !contains(err.Error(), tt.errorMsg) {
					t.Errorf("Validate() error = %q, want to contain %q", err.Error(), tt.errorMsg)
				}
			} else {
				if err != nil {
					t.Errorf("Validate() unexpected error = %v", err)
				}
			}
		})
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(substr) == 0 ||
		(len(s) > 0 && len(substr) > 0 && findSubstring(s, substr)))
}

func findSubstring(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

func TestIsFileTooLarge(t *testing.T) {
	tests := []struct {
		name    string
		size    int64
		maxSize int64
		want    bool
	}{
		{"no limit (zero)", 1000000, 0, false},
		{"no limit (negative)", 1000000, -1, false},
		{"under limit", 100, 1000, false},
		{"at limit", 1000, 1000, false},
		{"over limit", 1001, 1000, true},
		{"way over limit", 1000000, 1000, true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := IsFileTooLarge(tt.size, tt.maxSize)
			if got != tt.want {
				t.Errorf("IsFileTooLarge(%d, %d) = %v, want %v", tt.size, tt.maxSize, got, tt.want)
			}
		})
	}
}

func TestValidate_AdditionalCases(t *testing.T) {
	tests := []struct {
		name      string
		modify    func(*Config)
		wantError bool
		errorMsg  string
	}{
		{
			name:      "negative max file size",
			modify:    func(c *Config) { c.Analysis.MaxFileSize = -100 },
			wantError: true,
			errorMsg:  "max_file_size must be non-negative",
		},
		{
			name:      "cognitive complexity zero",
			modify:    func(c *Config) { c.Thresholds.CognitiveComplexity = 0 },
			wantError: true,
			errorMsg:  "cognitive_complexity must be at least 1",
		},
		{
			name:      "duplicate min lines zero",
			modify:    func(c *Config) { c.Thresholds.DuplicateMinLines = 0 },
			wantError: true,
			errorMsg:  "duplicate_min_lines must be at least 1",
		},
		{
			name:      "defect high risk negative",
			modify:    func(c *Config) { c.Thresholds.DefectHighRisk = -0.1 },
			wantError: true,
			errorMsg:  "defect_high_risk must be between 0 and 1",
		},
		{
			name:      "defect high risk over 1",
			modify:    func(c *Config) { c.Thresholds.DefectHighRisk = 1.5 },
			wantError: true,
			errorMsg:  "defect_high_risk must be between 0 and 1",
		},
		{
			name:      "TDG high risk negative",
			modify:    func(c *Config) { c.Thresholds.TDGHighRisk = -1 },
			wantError: true,
			errorMsg:  "tdg_high_risk must be non-negative",
		},
		{
			name:      "shingle size zero",
			modify:    func(c *Config) { c.Duplicates.ShingleSize = 0 },
			wantError: true,
			errorMsg:  "shingle_size must be at least 1",
		},
		{
			name:      "similarity threshold negative",
			modify:    func(c *Config) { c.Duplicates.SimilarityThreshold = -0.1 },
			wantError: true,
			errorMsg:  "similarity_threshold must be between 0 and 1",
		},
		{
			name:      "similarity threshold over 1",
			modify:    func(c *Config) { c.Duplicates.SimilarityThreshold = 1.5 },
			wantError: true,
			errorMsg:  "similarity_threshold must be between 0 and 1",
		},
		{
			name:      "hash functions zero",
			modify:    func(c *Config) { c.Duplicates.NumHashFunctions = 0 },
			wantError: true,
			errorMsg:  "num_hash_functions must be at least 1",
		},
		{
			name:      "bands zero",
			modify:    func(c *Config) { c.Duplicates.NumBands = 0 },
			wantError: true,
			errorMsg:  "num_bands must be at least 1",
		},
		{
			name:      "rows per band zero",
			modify:    func(c *Config) { c.Duplicates.RowsPerBand = 0 },
			wantError: true,
			errorMsg:  "rows_per_band must be at least 1",
		},
		{
			name: "valid hash relationship",
			modify: func(c *Config) {
				c.Duplicates.NumHashFunctions = 100
				c.Duplicates.NumBands = 20
				c.Duplicates.RowsPerBand = 5 // 20 * 5 = 100
			},
			wantError: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := DefaultConfig()
			tt.modify(cfg)

			err := cfg.Validate()

			if tt.wantError {
				if err == nil {
					t.Errorf("Validate() expected error containing %q, got nil", tt.errorMsg)
				} else if tt.errorMsg != "" && !contains(err.Error(), tt.errorMsg) {
					t.Errorf("Validate() error = %q, want to contain %q", err.Error(), tt.errorMsg)
				}
			} else {
				if err != nil {
					t.Errorf("Validate() unexpected error = %v", err)
				}
			}
		})
	}
}

func TestLoadUnsupportedExtension(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "omen.xml")

	content := `<config>invalid</config>`
	if err := os.WriteFile(configPath, []byte(content), 0644); err != nil {
		t.Fatalf("Failed to write config file: %v", err)
	}

	_, err := Load(configPath)
	if err == nil {
		t.Error("Load() should return error for unsupported file extension")
	}
}

func TestEffectiveWeights(t *testing.T) {
	tests := []struct {
		name           string
		enableCohesion bool
		weights        ScoreWeights
		wantCohesion   float64
	}{
		{
			name:           "enable_cohesion redistributes weights",
			enableCohesion: true,
			weights: ScoreWeights{
				Complexity:  0.30,
				Duplication: 0.25,
				SATD:        0.15,
				TDG:         0.10,
				Coupling:    0.15,
				Smells:      0.05,
				Cohesion:    0.0,
			},
			wantCohesion: DefaultCohesionWeight,
		},
		{
			name:           "disable_cohesion keeps original weights",
			enableCohesion: false,
			weights: ScoreWeights{
				Complexity:  0.30,
				Duplication: 0.25,
				SATD:        0.15,
				TDG:         0.10,
				Coupling:    0.15,
				Smells:      0.05,
				Cohesion:    0.0,
			},
			wantCohesion: 0.0,
		},
		{
			name:           "enable_cohesion with manual cohesion weight keeps it",
			enableCohesion: true,
			weights: ScoreWeights{
				Complexity:  0.25,
				Duplication: 0.20,
				SATD:        0.10,
				TDG:         0.10,
				Coupling:    0.15,
				Smells:      0.05,
				Cohesion:    0.15,
			},
			wantCohesion: 0.15,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := &ScoreConfig{
				EnableCohesion: tt.enableCohesion,
				Weights:        tt.weights,
			}
			effective := cfg.EffectiveWeights()
			if effective.Cohesion != tt.wantCohesion {
				t.Errorf("EffectiveWeights().Cohesion = %f, want %f", effective.Cohesion, tt.wantCohesion)
			}

			// Verify weights still sum to 1.0
			sum := effective.Complexity + effective.Duplication +
				effective.SATD + effective.TDG + effective.Coupling + effective.Smells + effective.Cohesion
			if sum < 0.99 || sum > 1.01 {
				t.Errorf("EffectiveWeights() sum = %f, want 1.0", sum)
			}
		})
	}
}
