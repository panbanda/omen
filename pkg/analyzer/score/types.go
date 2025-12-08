package score

import (
	"math"
	"time"
)

// Weights defines the weights for each component in the composite score.
// Note: Defect prediction is excluded from composite scoring because it requires
// git history (churn data) which isn't available during trend analysis. For
// consistency between `omen score` and `omen trend`, defect is analyzed separately
// via `omen analyze defect`.
type Weights struct {
	Complexity  float64 `json:"complexity" toml:"complexity"`
	Duplication float64 `json:"duplication" toml:"duplication"`
	SATD        float64 `json:"satd" toml:"satd"` // Self-Admitted Technical Debt (TODO/FIXME markers)
	TDG         float64 `json:"tdg" toml:"tdg"`   // Technical Debt Gradient (comprehensive debt score)
	Coupling    float64 `json:"coupling" toml:"coupling"`
	Smells      float64 `json:"smells" toml:"smells"`
	Cohesion    float64 `json:"cohesion" toml:"cohesion"` // Optional, for OO-heavy codebases
}

// DefaultWeights returns the default weights (must sum to 1.0).
func DefaultWeights() Weights {
	return Weights{
		Complexity:  0.25,
		Duplication: 0.20,
		SATD:        0.10,
		TDG:         0.15,
		Coupling:    0.10,
		Smells:      0.05,
		Cohesion:    0.15,
	}
}

// Thresholds defines minimum acceptable scores for each component.
type Thresholds struct {
	Score       int `json:"score" toml:"score"`
	Complexity  int `json:"complexity" toml:"complexity"`
	Duplication int `json:"duplication" toml:"duplication"`
	SATD        int `json:"satd" toml:"satd"`
	TDG         int `json:"tdg" toml:"tdg"`
	Coupling    int `json:"coupling" toml:"coupling"`
	Smells      int `json:"smells" toml:"smells"`
	Cohesion    int `json:"cohesion" toml:"cohesion"`
}

// ComponentScores holds the individual component scores (0-100 each).
type ComponentScores struct {
	Complexity  int `json:"complexity"`
	Duplication int `json:"duplication"`
	SATD        int `json:"satd"` // Self-Admitted Technical Debt
	TDG         int `json:"tdg"`  // Technical Debt Gradient
	Coupling    int `json:"coupling"`
	Smells      int `json:"smells"`
	Cohesion    int `json:"cohesion"`
}

// ThresholdResult tracks pass/fail status for a threshold check.
type ThresholdResult struct {
	Min    int  `json:"min"`
	Passed bool `json:"passed"`
}

// Result represents the complete score analysis result.
type Result struct {
	Score            int                        `json:"score"`
	Components       ComponentScores            `json:"components"`
	CohesionIncluded bool                       `json:"cohesion_included"` // True if cohesion weight > 0
	Weights          Weights                    `json:"weights"`
	FilesAnalyzed    int                        `json:"files_analyzed"`
	Thresholds       map[string]ThresholdResult `json:"thresholds,omitempty"`
	Passed           bool                       `json:"passed"`
	Timestamp        time.Time                  `json:"timestamp"`
	Commit           string                     `json:"commit,omitempty"`
}

// ComputeComposite calculates the weighted composite score.
func (r *Result) ComputeComposite() {
	weighted := float64(r.Components.Complexity)*r.Weights.Complexity +
		float64(r.Components.Duplication)*r.Weights.Duplication +
		float64(r.Components.SATD)*r.Weights.SATD +
		float64(r.Components.TDG)*r.Weights.TDG +
		float64(r.Components.Coupling)*r.Weights.Coupling +
		float64(r.Components.Smells)*r.Weights.Smells +
		float64(r.Components.Cohesion)*r.Weights.Cohesion

	r.CohesionIncluded = r.Weights.Cohesion > 0

	r.Score = int(math.Round(weighted))
	if r.Score > 100 {
		r.Score = 100
	}
	if r.Score < 0 {
		r.Score = 0
	}
}

// CheckThresholds evaluates all thresholds and sets Passed status.
func (r *Result) CheckThresholds(t Thresholds) {
	r.Thresholds = make(map[string]ThresholdResult)
	r.Passed = true

	check := func(name string, actual, min int) {
		passed := min == 0 || actual >= min
		r.Thresholds[name] = ThresholdResult{Min: min, Passed: passed}
		if !passed {
			r.Passed = false
		}
	}

	check("score", r.Score, t.Score)
	check("complexity", r.Components.Complexity, t.Complexity)
	check("duplication", r.Components.Duplication, t.Duplication)
	check("satd", r.Components.SATD, t.SATD)
	check("tdg", r.Components.TDG, t.TDG)
	check("coupling", r.Components.Coupling, t.Coupling)
	check("smells", r.Components.Smells, t.Smells)
	check("cohesion", r.Components.Cohesion, t.Cohesion)
}

// Normalizer functions are in normalize.go with detailed documentation
// explaining the rationale and calibration for each scoring algorithm.
