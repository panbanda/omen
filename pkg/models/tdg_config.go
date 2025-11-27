package models

import (
	"encoding/json"
	"os"
)

// WeightConfig defines the weight for each TDG metric component.
type WeightConfig struct {
	StructuralComplexity float32 `json:"structural_complexity" toml:"structural_complexity"`
	SemanticComplexity   float32 `json:"semantic_complexity" toml:"semantic_complexity"`
	Duplication          float32 `json:"duplication" toml:"duplication"`
	Coupling             float32 `json:"coupling" toml:"coupling"`
	Documentation        float32 `json:"documentation" toml:"documentation"`
	Consistency          float32 `json:"consistency" toml:"consistency"`
}

// DefaultWeightConfig returns the default weight configuration.
func DefaultWeightConfig() WeightConfig {
	return WeightConfig{
		StructuralComplexity: 25.0,
		SemanticComplexity:   20.0,
		Duplication:          20.0,
		Coupling:             15.0,
		Documentation:        10.0,
		Consistency:          10.0,
	}
}

// ThresholdConfig defines thresholds for TDG analysis.
type ThresholdConfig struct {
	MaxCyclomaticComplexity uint32  `json:"max_cyclomatic_complexity" toml:"max_cyclomatic_complexity"`
	MaxCognitiveComplexity  uint32  `json:"max_cognitive_complexity" toml:"max_cognitive_complexity"`
	MaxNestingDepth         uint32  `json:"max_nesting_depth" toml:"max_nesting_depth"`
	MinTokenSequence        uint32  `json:"min_token_sequence" toml:"min_token_sequence"`
	SimilarityThreshold     float32 `json:"similarity_threshold" toml:"similarity_threshold"`
	MaxCoupling             uint32  `json:"max_coupling" toml:"max_coupling"`
	MinDocCoverage          float32 `json:"min_doc_coverage" toml:"min_doc_coverage"`
}

// DefaultThresholdConfig returns enterprise-standard thresholds.
func DefaultThresholdConfig() ThresholdConfig {
	return ThresholdConfig{
		MaxCyclomaticComplexity: 30,   // Enterprise standard
		MaxCognitiveComplexity:  25,   // Reasonable threshold
		MaxNestingDepth:         4,    // Allow reasonable nesting
		MinTokenSequence:        50,   // Minimum tokens for duplication
		SimilarityThreshold:     0.85, // 85% similarity threshold
		MaxCoupling:             15,   // Realistic coupling limit
		MinDocCoverage:          0.75, // Balanced target
	}
}

// PenaltyCurve defines how penalties are applied.
type PenaltyCurve string

const (
	PenaltyCurveLinear      PenaltyCurve = "linear"
	PenaltyCurveLogarithmic PenaltyCurve = "logarithmic"
	PenaltyCurveQuadratic   PenaltyCurve = "quadratic"
	PenaltyCurveExponential PenaltyCurve = "exponential"
)

// Apply applies the penalty curve to a value.
func (pc PenaltyCurve) Apply(value, base float32) float32 {
	switch pc {
	case PenaltyCurveLinear:
		return value * base
	case PenaltyCurveLogarithmic:
		if value > 1.0 {
			return ln32(value) * base
		}
		return 0
	case PenaltyCurveQuadratic:
		return value * value * base
	case PenaltyCurveExponential:
		return exp32(value) * base
	default:
		return value * base
	}
}

// ln32 computes natural log for float32
func ln32(x float32) float32 {
	return float32(2.302585) * log10_32(x) // ln(x) = ln(10) * log10(x)
}

// log10_32 computes log10 for float32 using a simple approximation
func log10_32(x float32) float32 {
	if x <= 0 {
		return 0
	}
	// Simple approximation using integer part
	var count float32
	for x >= 10 {
		x /= 10
		count++
	}
	for x < 1 {
		x *= 10
		count--
	}
	// Linear interpolation between 1 and 10
	return count + (x-1)/9
}

// exp32 computes e^x for float32 using Taylor series approximation
func exp32(x float32) float32 {
	if x > 20 {
		return 485165195.4 // Approximate cap
	}
	if x < -20 {
		return 0
	}
	// Taylor series approximation: e^x = 1 + x + x^2/2! + x^3/3! + ...
	result := float32(1.0)
	term := float32(1.0)
	for i := 1; i <= 10; i++ {
		term *= x / float32(i)
		result += term
	}
	return result
}

// PenaltyConfig defines penalty curves for each metric.
type PenaltyConfig struct {
	ComplexityPenaltyBase   PenaltyCurve `json:"complexity_penalty_base" toml:"complexity_penalty_base"`
	DuplicationPenaltyCurve PenaltyCurve `json:"duplication_penalty_curve" toml:"duplication_penalty_curve"`
	CouplingPenaltyCurve    PenaltyCurve `json:"coupling_penalty_curve" toml:"coupling_penalty_curve"`
}

// DefaultPenaltyConfig returns the default penalty configuration.
func DefaultPenaltyConfig() PenaltyConfig {
	return PenaltyConfig{
		ComplexityPenaltyBase:   PenaltyCurveLogarithmic,
		DuplicationPenaltyCurve: PenaltyCurveLinear,
		CouplingPenaltyCurve:    PenaltyCurveQuadratic,
	}
}

// LanguageOverride defines language-specific overrides.
type LanguageOverride struct {
	MaxCognitiveComplexity *uint32  `json:"max_cognitive_complexity,omitempty" toml:"max_cognitive_complexity,omitempty"`
	MinDocCoverage         *float32 `json:"min_doc_coverage,omitempty" toml:"min_doc_coverage,omitempty"`
	EnforceErrorCheck      *bool    `json:"enforce_error_check,omitempty" toml:"enforce_error_check,omitempty"`
	MaxFunctionLength      *uint32  `json:"max_function_length,omitempty" toml:"max_function_length,omitempty"`
}

// TdgConfig is the TDG configuration.
type TdgConfig struct {
	Weights           WeightConfig                `json:"weights" toml:"weights"`
	Thresholds        ThresholdConfig             `json:"thresholds" toml:"thresholds"`
	Penalties         PenaltyConfig               `json:"penalties" toml:"penalties"`
	LanguageOverrides map[string]LanguageOverride `json:"language_overrides,omitempty" toml:"language_overrides,omitempty"`
}

// DefaultTdgConfig returns the default TDG configuration.
func DefaultTdgConfig() TdgConfig {
	return TdgConfig{
		Weights:           DefaultWeightConfig(),
		Thresholds:        DefaultThresholdConfig(),
		Penalties:         DefaultPenaltyConfig(),
		LanguageOverrides: make(map[string]LanguageOverride),
	}
}

// LoadTdgConfig loads configuration from a JSON file.
func LoadTdgConfig(path string) (TdgConfig, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return DefaultTdgConfig(), err
	}

	var config TdgConfig
	if err := json.Unmarshal(data, &config); err != nil {
		return DefaultTdgConfig(), err
	}

	return config, nil
}
