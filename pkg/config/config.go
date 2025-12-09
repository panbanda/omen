package config

import (
	"errors"
	"fmt"
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
	Analysis AnalysisConfig `koanf:"analysis" toml:"analysis"`

	// Thresholds for various metrics
	Thresholds ThresholdConfig `koanf:"thresholds" toml:"thresholds"`

	// Duplicate detection settings
	Duplicates DuplicateConfig `koanf:"duplicates" toml:"duplicates"`

	// File exclusion patterns
	Exclude ExcludeConfig `koanf:"exclude" toml:"exclude"`

	// Cache settings
	Cache CacheConfig `koanf:"cache" toml:"cache"`

	// Output settings
	Output OutputConfig `koanf:"output" toml:"output"`

	// Feature flag settings
	FeatureFlags FeatureFlagConfig `koanf:"feature_flags" toml:"feature_flags"`

	// Score settings
	Score ScoreConfig `koanf:"score" toml:"score"`
}

// AnalysisConfig controls which analyzers run.
type AnalysisConfig struct {
	Complexity  bool  `koanf:"complexity" toml:"complexity"`
	SATD        bool  `koanf:"satd" toml:"satd"`
	DeadCode    bool  `koanf:"dead_code" toml:"dead_code"`
	Churn       bool  `koanf:"churn" toml:"churn"`
	Duplicates  bool  `koanf:"duplicates" toml:"duplicates"`
	Defect      bool  `koanf:"defect" toml:"defect"`
	TDG         bool  `koanf:"tdg" toml:"tdg"`
	Graph       bool  `koanf:"graph" toml:"graph"`
	LintHotspot bool  `koanf:"lint_hotspot" toml:"lint_hotspot"`
	Context     bool  `koanf:"context" toml:"context"`
	ChurnDays   int   `koanf:"churn_days" toml:"churn_days"`
	MaxFileSize int64 `koanf:"max_file_size" toml:"max_file_size"` // Maximum file size in bytes (0 = no limit)
}

// ThresholdConfig defines metric thresholds.
type ThresholdConfig struct {
	CyclomaticComplexity int     `koanf:"cyclomatic_complexity" toml:"cyclomatic_complexity"`
	CognitiveComplexity  int     `koanf:"cognitive_complexity" toml:"cognitive_complexity"`
	DuplicateMinLines    int     `koanf:"duplicate_min_lines" toml:"duplicate_min_lines"`
	DuplicateSimilarity  float64 `koanf:"duplicate_similarity" toml:"duplicate_similarity"`
	DeadCodeConfidence   float64 `koanf:"dead_code_confidence" toml:"dead_code_confidence"`
	DefectHighRisk       float64 `koanf:"defect_high_risk" toml:"defect_high_risk"`
	TDGHighRisk          float64 `koanf:"tdg_high_risk" toml:"tdg_high_risk"`
}

// DuplicateConfig defines duplicate detection settings (pmat-compatible).
type DuplicateConfig struct {
	MinTokens            int     `koanf:"min_tokens" toml:"min_tokens"`
	SimilarityThreshold  float64 `koanf:"similarity_threshold" toml:"similarity_threshold"`
	ShingleSize          int     `koanf:"shingle_size" toml:"shingle_size"`
	NumHashFunctions     int     `koanf:"num_hash_functions" toml:"num_hash_functions"`
	NumBands             int     `koanf:"num_bands" toml:"num_bands"`
	RowsPerBand          int     `koanf:"rows_per_band" toml:"rows_per_band"`
	NormalizeIdentifiers bool    `koanf:"normalize_identifiers" toml:"normalize_identifiers"`
	NormalizeLiterals    bool    `koanf:"normalize_literals" toml:"normalize_literals"`
	IgnoreComments       bool    `koanf:"ignore_comments" toml:"ignore_comments"`
	MinGroupSize         int     `koanf:"min_group_size" toml:"min_group_size"`
}

// ExcludeConfig defines file exclusion patterns using gitignore-style syntax.
// All patterns in the Patterns list are parsed as gitignore patterns and combined
// with the repository's .gitignore file (when Gitignore is true).
type ExcludeConfig struct {
	// Patterns uses gitignore syntax for excluding files:
	//   - "*_test.go"     matches any file ending in _test.go
	//   - "vendor/"       matches the vendor directory
	//   - "*.min.js"      matches minified JS files
	//   - "cmd/**/main.go" matches main.go in any subdirectory of cmd
	//   - "!important.go" negates a previous pattern (include the file)
	Patterns []string `koanf:"patterns" toml:"patterns"`

	// Gitignore controls whether to also respect .gitignore files.
	// When true, patterns from .gitignore are combined with Patterns.
	Gitignore bool `koanf:"gitignore" toml:"gitignore"`
}

// CacheConfig controls caching behavior.
type CacheConfig struct {
	Enabled bool   `koanf:"enabled" toml:"enabled"`
	Dir     string `koanf:"dir" toml:"dir"`
	TTL     int    `koanf:"ttl" toml:"ttl"` // TTL in hours
}

// OutputConfig controls output formatting.
type OutputConfig struct {
	Format  string `koanf:"format" toml:"format"` // text, json, markdown
	Color   bool   `koanf:"color" toml:"color"`
	Verbose bool   `koanf:"verbose" toml:"verbose"`
}

// FeatureFlagConfig controls feature flag detection settings.
type FeatureFlagConfig struct {
	// Providers to detect (empty = all)
	Providers []string `koanf:"providers" toml:"providers"`

	// Custom providers with inline query definitions
	CustomProviders []CustomProviderConfig `koanf:"custom_providers" toml:"custom_providers"`

	// Expected TTL settings (days)
	ExpectedTTL FeatureFlagTTLConfig `koanf:"expected_ttl" toml:"expected_ttl"`
}

// CustomProviderConfig defines a custom feature flag provider with query patterns.
type CustomProviderConfig struct {
	// Name of the custom provider (e.g., "feature", "custom-flipper")
	Name string `koanf:"name" toml:"name"`

	// Languages this provider applies to (e.g., ["ruby", "javascript"])
	Languages []string `koanf:"languages" toml:"languages"`

	// Tree-sitter query pattern (.scm format)
	// Must capture @flag_key for the flag name
	Query string `koanf:"query" toml:"query"`
}

// FeatureFlagTTLConfig defines expected time-to-live settings.
type FeatureFlagTTLConfig struct {
	Release    int `koanf:"release" toml:"release"`       // days, default 14
	Experiment int `koanf:"experiment" toml:"experiment"` // days, default 90
}

// ScoreConfig defines repository score settings.
type ScoreConfig struct {
	EnableCohesion bool            `koanf:"enable_cohesion" toml:"enable_cohesion"` // Include cohesion in composite score
	Weights        ScoreWeights    `koanf:"weights" toml:"weights"`
	Thresholds     ScoreThresholds `koanf:"thresholds" toml:"thresholds"`
}

// ScoreWeights defines component weights (must sum to 1.0).
// Note: Defect prediction is excluded from composite scoring because it requires
// git history (churn data) which isn't available during trend analysis.
type ScoreWeights struct {
	Complexity  float64 `koanf:"complexity" toml:"complexity"`
	Duplication float64 `koanf:"duplication" toml:"duplication"`
	SATD        float64 `koanf:"satd" toml:"satd"` // Self-Admitted Technical Debt (TODO/FIXME markers)
	TDG         float64 `koanf:"tdg" toml:"tdg"`   // Technical Debt Gradient (comprehensive debt score)
	Coupling    float64 `koanf:"coupling" toml:"coupling"`
	Smells      float64 `koanf:"smells" toml:"smells"`
	Cohesion    float64 `koanf:"cohesion" toml:"cohesion"` // Optional, for OO-heavy codebases
}

// ScoreThresholds defines minimum acceptable scores.
type ScoreThresholds struct {
	Score       int `koanf:"score" toml:"score"`
	Complexity  int `koanf:"complexity" toml:"complexity"`
	Duplication int `koanf:"duplication" toml:"duplication"`
	SATD        int `koanf:"satd" toml:"satd"`
	TDG         int `koanf:"tdg" toml:"tdg"`
	Coupling    int `koanf:"coupling" toml:"coupling"`
	Smells      int `koanf:"smells" toml:"smells"`
	Cohesion    int `koanf:"cohesion" toml:"cohesion"`
}

// DefaultCohesionWeight is the weight used for cohesion when enable_cohesion is true.
const DefaultCohesionWeight = 0.15

// EffectiveWeights returns weights adjusted for enable_cohesion.
// When enable_cohesion is true and cohesion weight is 0, it redistributes weights
// to include cohesion at DefaultCohesionWeight.
func (c *ScoreConfig) EffectiveWeights() ScoreWeights {
	if !c.EnableCohesion || c.Weights.Cohesion > 0 {
		return c.Weights
	}

	// Scale down existing weights to make room for cohesion
	scale := 1.0 - DefaultCohesionWeight
	return ScoreWeights{
		Complexity:  c.Weights.Complexity * scale,
		Duplication: c.Weights.Duplication * scale,
		SATD:        c.Weights.SATD * scale,
		TDG:         c.Weights.TDG * scale,
		Coupling:    c.Weights.Coupling * scale,
		Smells:      c.Weights.Smells * scale,
		Cohesion:    DefaultCohesionWeight,
	}
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
			ChurnDays:   30,
			MaxFileSize: 10 * 1024 * 1024, // 10 MB default
		},
		Thresholds: ThresholdConfig{
			CyclomaticComplexity: 10,
			CognitiveComplexity:  15,
			DuplicateMinLines:    6,
			DuplicateSimilarity:  0.8,
			DeadCodeConfidence:   0.8,
			DefectHighRisk:       0.6,
			TDGHighRisk:          2.5, // Critical threshold on 0-5 scale
		},
		Duplicates: DuplicateConfig{
			MinTokens:            50,
			SimilarityThreshold:  0.70,
			ShingleSize:          5,
			NumHashFunctions:     200,
			NumBands:             20,
			RowsPerBand:          10,
			NormalizeIdentifiers: true,
			NormalizeLiterals:    true,
			IgnoreComments:       true,
			MinGroupSize:         2,
		},
		Exclude: ExcludeConfig{
			Patterns: []string{
				// Test files
				"*_test.go",
				"*_test.ts",
				"*_test.py",
				"*.spec.ts",
				"*.spec.js",
				"*_spec.rb",
				"**/*_test/**",
				"**/test/**",
				"**/tests/**",
				"**/spec/**",
				// Minified assets
				"*.min.js",
				"*.min.css",
				// Lock files
				"*.lock",
				"go.sum",
				// Vendor directories
				"vendor/",
				"node_modules/",
				"third_party/",
				"external/",
				// Build/output directories
				".git/",
				".omen/",
				"dist/",
				"build/",
				"target/",
				"out/",
				"bin/",
				// Python
				"__pycache__/",
				".venv/",
				"venv/",
				"site-packages/",
				// Ruby
				".bundle/",
				"sorbet/",
				// JavaScript/Node
				".yarn/", // Yarn 2+ PnP releases and plugins
				// Coverage/test output
				"coverage/",
				".nyc_output/",
				// Auto-generated code
				"**/mocks/",
				"**/*.gen.go",
				"**/*.generated.go",
				"**/*.pb.go",
				"**/generated/",
				"**/gen/",
				"**/*.auto.ts",
				"**/*.g.dart",
				"*_generated.rb",
				// Schema/migration files (often auto-generated)
				"**/schema.rb",
				"**/structure.sql",
				// IDE/editor directories
				".idea/",
				".vscode/",
				".vs/",
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
		FeatureFlags: FeatureFlagConfig{
			Providers:       []string{}, // empty = all providers
			CustomProviders: []CustomProviderConfig{},
			ExpectedTTL: FeatureFlagTTLConfig{
				Release:    14,
				Experiment: 90,
			},
		},
		Score: ScoreConfig{
			Weights: ScoreWeights{
				Complexity:  0.25,
				Duplication: 0.20,
				SATD:        0.10,
				TDG:         0.15,
				Coupling:    0.10,
				Smells:      0.05,
				Cohesion:    0.15,
			},
			Thresholds: ScoreThresholds{}, // All zeros = no enforcement
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

// FindConfigFile searches for a config file in standard locations.
// Returns the path if found, or empty string if not found.
func FindConfigFile() string {
	configNames := []string{
		"omen.toml",
		"omen.yaml",
		"omen.yml",
		"omen.json",
	}

	searchDirs := []string{".", ".omen"}

	for _, dir := range searchDirs {
		for _, name := range configNames {
			path := filepath.Join(dir, name)
			if _, err := os.Stat(path); err == nil {
				return path
			}
		}
	}
	return ""
}

// LoadOption configures how configuration is loaded.
type LoadOption func(*loadOptions)

type loadOptions struct {
	path string
}

// WithPath specifies an explicit config file path.
// If the path doesn't exist, an error is returned.
func WithPath(path string) LoadOption {
	return func(o *loadOptions) {
		o.path = path
	}
}

// LoadResult contains the loaded configuration and metadata.
type LoadResult struct {
	Config *Config
	Source string // Path to the config file, empty if using defaults
}

// LoadConfig loads configuration with the provided options.
// If no path is specified, it searches standard locations.
// Returns defaults if no config file is found.
// Always validates the config before returning.
func LoadConfig(opts ...LoadOption) (*LoadResult, error) {
	o := &loadOptions{}
	for _, opt := range opts {
		opt(o)
	}

	var cfg *Config
	var source string
	var err error

	if o.path != "" {
		if _, statErr := os.Stat(o.path); os.IsNotExist(statErr) {
			return nil, fmt.Errorf("config file not found: %s", o.path)
		}
		cfg, err = Load(o.path)
		if err != nil {
			return nil, fmt.Errorf("failed to load %s: %w", o.path, err)
		}
		source = o.path
	} else {
		source = FindConfigFile()
		if source == "" {
			cfg = DefaultConfig()
		} else {
			cfg, err = Load(source)
			if err != nil {
				return nil, fmt.Errorf("failed to load %s: %w", source, err)
			}
		}
	}

	if validationErr := cfg.Validate(); validationErr != nil {
		return nil, fmt.Errorf("config validation failed: %w", validationErr)
	}

	return &LoadResult{Config: cfg, Source: source}, nil
}

// LoadOrDefault loads config from standard locations or returns defaults.
// Returns an error if validation fails.
func LoadOrDefault() (*Config, error) {
	result, err := LoadConfig()
	if err != nil {
		if FindConfigFile() == "" {
			return DefaultConfig(), nil
		}
		return nil, err
	}
	return result.Config, nil
}

// ShouldExclude is deprecated. Use the scanner's gitignore-based matching instead.
// This method is kept for backward compatibility but only does basic pattern matching.
func (c *Config) ShouldExclude(path string) bool {
	// Basic pattern matching for backward compatibility
	// The scanner now handles full gitignore-style matching
	base := filepath.Base(path)
	for _, pattern := range c.Exclude.Patterns {
		// Skip directory patterns (handled by scanner)
		if strings.HasSuffix(pattern, "/") {
			continue
		}
		// Skip glob patterns with path separators (handled by scanner)
		if strings.Contains(pattern, "/") {
			continue
		}
		if matched, _ := filepath.Match(pattern, base); matched {
			return true
		}
	}
	return false
}

// ErrFileTooLarge is returned when a file exceeds the configured size limit.
var ErrFileTooLarge = errors.New("file exceeds maximum size limit")

// IsFileTooLarge checks if a file exceeds the configured maximum size.
// Returns true if the file is too large, false otherwise.
// If maxSize is 0, no limit is enforced.
func IsFileTooLarge(size int64, maxSize int64) bool {
	if maxSize <= 0 {
		return false
	}
	return size > maxSize
}

// Validate checks that all config values are within acceptable ranges.
// Returns an error describing any validation failures.
func (c *Config) Validate() error {
	var errs []error

	// Analysis config validation
	if c.Analysis.ChurnDays < 1 {
		errs = append(errs, errors.New("analysis.churn_days must be at least 1"))
	}
	if c.Analysis.ChurnDays > 3650 { // 10 years max
		errs = append(errs, errors.New("analysis.churn_days must be at most 3650"))
	}
	if c.Analysis.MaxFileSize < 0 {
		errs = append(errs, errors.New("analysis.max_file_size must be non-negative"))
	}

	// Threshold validation
	if c.Thresholds.CyclomaticComplexity < 1 {
		errs = append(errs, errors.New("thresholds.cyclomatic_complexity must be at least 1"))
	}
	if c.Thresholds.CognitiveComplexity < 1 {
		errs = append(errs, errors.New("thresholds.cognitive_complexity must be at least 1"))
	}
	if c.Thresholds.DuplicateMinLines < 1 {
		errs = append(errs, errors.New("thresholds.duplicate_min_lines must be at least 1"))
	}
	if c.Thresholds.DuplicateSimilarity < 0 || c.Thresholds.DuplicateSimilarity > 1 {
		errs = append(errs, errors.New("thresholds.duplicate_similarity must be between 0 and 1"))
	}
	if c.Thresholds.DeadCodeConfidence < 0 || c.Thresholds.DeadCodeConfidence > 1 {
		errs = append(errs, errors.New("thresholds.dead_code_confidence must be between 0 and 1"))
	}
	if c.Thresholds.DefectHighRisk < 0 || c.Thresholds.DefectHighRisk > 1 {
		errs = append(errs, errors.New("thresholds.defect_high_risk must be between 0 and 1"))
	}
	if c.Thresholds.TDGHighRisk < 0 {
		errs = append(errs, errors.New("thresholds.tdg_high_risk must be non-negative"))
	}

	// Duplicate config validation
	if c.Duplicates.MinTokens < 1 {
		errs = append(errs, errors.New("duplicates.min_tokens must be at least 1"))
	}
	if c.Duplicates.SimilarityThreshold < 0 || c.Duplicates.SimilarityThreshold > 1 {
		errs = append(errs, errors.New("duplicates.similarity_threshold must be between 0 and 1"))
	}
	if c.Duplicates.ShingleSize < 1 {
		errs = append(errs, errors.New("duplicates.shingle_size must be at least 1"))
	}
	if c.Duplicates.NumHashFunctions < 1 {
		errs = append(errs, errors.New("duplicates.num_hash_functions must be at least 1"))
	}
	if c.Duplicates.NumBands < 1 {
		errs = append(errs, errors.New("duplicates.num_bands must be at least 1"))
	}
	if c.Duplicates.RowsPerBand < 1 {
		errs = append(errs, errors.New("duplicates.rows_per_band must be at least 1"))
	}
	if c.Duplicates.MinGroupSize < 2 {
		errs = append(errs, errors.New("duplicates.min_group_size must be at least 2"))
	}

	// Validate relationship: NumHashFunctions should equal NumBands * RowsPerBand
	if c.Duplicates.NumHashFunctions != c.Duplicates.NumBands*c.Duplicates.RowsPerBand {
		errs = append(errs, fmt.Errorf(
			"duplicates.num_hash_functions (%d) should equal num_bands (%d) * rows_per_band (%d) = %d",
			c.Duplicates.NumHashFunctions,
			c.Duplicates.NumBands,
			c.Duplicates.RowsPerBand,
			c.Duplicates.NumBands*c.Duplicates.RowsPerBand,
		))
	}

	// Cache config validation
	if c.Cache.TTL < 0 {
		errs = append(errs, errors.New("cache.ttl must be non-negative"))
	}

	// Score config validation - validate effective weights (after enable_cohesion adjustment)
	effectiveWeights := c.Score.EffectiveWeights()
	weightsSum := effectiveWeights.Complexity + effectiveWeights.Duplication +
		effectiveWeights.SATD + effectiveWeights.TDG +
		effectiveWeights.Coupling + effectiveWeights.Smells +
		effectiveWeights.Cohesion
	if weightsSum < 0.99 || weightsSum > 1.01 {
		errs = append(errs, fmt.Errorf("score.weights must sum to 1.0, got %f", weightsSum))
	}

	// Validate individual weights are non-negative
	if effectiveWeights.Complexity < 0 || effectiveWeights.Duplication < 0 ||
		effectiveWeights.SATD < 0 || effectiveWeights.TDG < 0 ||
		effectiveWeights.Coupling < 0 || effectiveWeights.Smells < 0 ||
		effectiveWeights.Cohesion < 0 {
		errs = append(errs, errors.New("score.weights values must be non-negative"))
	}

	// Validate thresholds are in 0-100 range
	validateThreshold := func(name string, val int) {
		if val < 0 || val > 100 {
			errs = append(errs, fmt.Errorf("score.thresholds.%s must be between 0 and 100", name))
		}
	}
	validateThreshold("score", c.Score.Thresholds.Score)
	validateThreshold("complexity", c.Score.Thresholds.Complexity)
	validateThreshold("duplication", c.Score.Thresholds.Duplication)
	validateThreshold("satd", c.Score.Thresholds.SATD)
	validateThreshold("tdg", c.Score.Thresholds.TDG)
	validateThreshold("coupling", c.Score.Thresholds.Coupling)
	validateThreshold("smells", c.Score.Thresholds.Smells)
	validateThreshold("cohesion", c.Score.Thresholds.Cohesion)

	if len(errs) > 0 {
		return errors.Join(errs...)
	}
	return nil
}
