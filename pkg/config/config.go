package config

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/knadh/koanf/parsers/json"
	"github.com/knadh/koanf/parsers/toml"
	"github.com/knadh/koanf/parsers/yaml"
	"github.com/knadh/koanf/providers/file"
	"github.com/knadh/koanf/v2"
)

// Config holds all configuration options for omen.
type Config struct {
	// Analysis settings
	Analysis AnalysisConfig `koanf:"analysis"`

	// Thresholds for various metrics
	Thresholds ThresholdConfig `koanf:"thresholds"`

	// File exclusion patterns
	Exclude ExcludeConfig `koanf:"exclude"`

	// Cache settings
	Cache CacheConfig `koanf:"cache"`

	// Output settings
	Output OutputConfig `koanf:"output"`
}

// AnalysisConfig controls which analyzers run.
type AnalysisConfig struct {
	Complexity  bool `koanf:"complexity"`
	SATD        bool `koanf:"satd"`
	DeadCode    bool `koanf:"dead_code"`
	Churn       bool `koanf:"churn"`
	Duplicates  bool `koanf:"duplicates"`
	Defect      bool `koanf:"defect"`
	TDG         bool `koanf:"tdg"`
	Graph       bool `koanf:"graph"`
	LintHotspot bool `koanf:"lint_hotspot"`
	Context     bool `koanf:"context"`
	ChurnDays   int  `koanf:"churn_days"`
}

// ThresholdConfig defines metric thresholds.
type ThresholdConfig struct {
	CyclomaticComplexity int     `koanf:"cyclomatic_complexity"`
	CognitiveComplexity  int     `koanf:"cognitive_complexity"`
	DuplicateMinLines    int     `koanf:"duplicate_min_lines"`
	DuplicateSimilarity  float64 `koanf:"duplicate_similarity"`
	DeadCodeConfidence   float64 `koanf:"dead_code_confidence"`
	DefectHighRisk       float64 `koanf:"defect_high_risk"`
	TDGHighRisk          float64 `koanf:"tdg_high_risk"`
}

// ExcludeConfig defines file exclusion patterns.
type ExcludeConfig struct {
	Patterns   []string `koanf:"patterns"`
	Extensions []string `koanf:"extensions"`
	Dirs       []string `koanf:"dirs"`
	Gitignore  bool     `koanf:"gitignore"`
}

// CacheConfig controls caching behavior.
type CacheConfig struct {
	Enabled bool   `koanf:"enabled"`
	Dir     string `koanf:"dir"`
	TTL     int    `koanf:"ttl"` // TTL in hours
}

// OutputConfig controls output formatting.
type OutputConfig struct {
	Format  string `koanf:"format"` // text, json, markdown
	Color   bool   `koanf:"color"`
	Verbose bool   `koanf:"verbose"`
}

// DefaultConfig returns a config with sensible defaults.
func DefaultConfig() *Config {
	return &Config{
		Analysis: AnalysisConfig{
			Complexity:  true,
			SATD:        true,
			DeadCode:    true,
			Churn:       true,
			Duplicates:  true,
			Defect:      true,
			TDG:         true,
			Graph:       true,
			LintHotspot: true,
			Context:     true,
			ChurnDays:   90,
		},
		Thresholds: ThresholdConfig{
			CyclomaticComplexity: 10,
			CognitiveComplexity:  15,
			DuplicateMinLines:    6,
			DuplicateSimilarity:  0.8,
			DeadCodeConfidence:   0.8,
			DefectHighRisk:       0.6,
			TDGHighRisk:          3.0,
		},
		Exclude: ExcludeConfig{
			Patterns: []string{
				"*_test.go",
				"*_test.ts",
				"*_test.py",
				"*.min.js",
				"*.min.css",
			},
			Extensions: []string{
				".lock",
				".sum",
			},
			Dirs: []string{
				"vendor",
				"node_modules",
				".git",
				".omen",
				"dist",
				"build",
				"__pycache__",
			},
			Gitignore: true,
		},
		Cache: CacheConfig{
			Enabled: true,
			Dir:     ".omen/cache",
			TTL:     24,
		},
		Output: OutputConfig{
			Format:  "text",
			Color:   true,
			Verbose: false,
		},
	}
}

// Load loads configuration from a file.
func Load(path string) (*Config, error) {
	k := koanf.New(".")
	cfg := DefaultConfig()

	// Determine parser based on extension
	var parser koanf.Parser
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".toml":
		parser = toml.Parser()
	case ".yaml", ".yml":
		parser = yaml.Parser()
	case ".json":
		parser = json.Parser()
	default:
		// Try to detect from content or default to TOML
		parser = toml.Parser()
	}

	// Load the config file
	if err := k.Load(file.Provider(path), parser); err != nil {
		return nil, err
	}

	// Unmarshal into config struct
	if err := k.Unmarshal("", cfg); err != nil {
		return nil, err
	}

	return cfg, nil
}

// LoadOrDefault tries to load config from standard locations or returns defaults.
func LoadOrDefault() *Config {
	// Standard config file names to search for
	configNames := []string{
		"omen.toml",
		"omen.yaml",
		"omen.yml",
		"omen.json",
		".omen.toml",
		".omen.yaml",
		".omen.yml",
		".omen.json",
	}

	// Search in current directory and .omen directory
	searchDirs := []string{".", ".omen"}

	for _, dir := range searchDirs {
		for _, name := range configNames {
			path := filepath.Join(dir, name)
			if _, err := os.Stat(path); err == nil {
				cfg, err := Load(path)
				if err == nil {
					return cfg
				}
			}
		}
	}

	return DefaultConfig()
}

// ShouldExclude checks if a path should be excluded from analysis.
func (c *Config) ShouldExclude(path string) bool {
	// Check directory exclusions
	for _, dir := range c.Exclude.Dirs {
		if strings.Contains(path, string(filepath.Separator)+dir+string(filepath.Separator)) ||
			strings.HasPrefix(path, dir+string(filepath.Separator)) {
			return true
		}
	}

	// Check extension exclusions
	ext := filepath.Ext(path)
	for _, excludeExt := range c.Exclude.Extensions {
		if ext == excludeExt {
			return true
		}
	}

	// Check pattern exclusions
	base := filepath.Base(path)
	for _, pattern := range c.Exclude.Patterns {
		if matched, _ := filepath.Match(pattern, base); matched {
			return true
		}
	}

	return false
}
