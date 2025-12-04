package score

import (
	"math"
	"time"
)

// Grade represents a letter grade from A+ to F.
type Grade string

const (
	GradeAPlus  Grade = "A+"
	GradeA      Grade = "A"
	GradeAMinus Grade = "A-"
	GradeBPlus  Grade = "B+"
	GradeB      Grade = "B"
	GradeBMinus Grade = "B-"
	GradeCPlus  Grade = "C+"
	GradeC      Grade = "C"
	GradeCMinus Grade = "C-"
	GradeDPlus  Grade = "D+"
	GradeD      Grade = "D"
	GradeDMinus Grade = "D-"
	GradeF      Grade = "F"
)

// GradeFromScore converts a 0-100 score to a letter grade.
// Uses standard academic grading scale.
func GradeFromScore(score int) Grade {
	switch {
	case score >= 97:
		return GradeAPlus
	case score >= 93:
		return GradeA
	case score >= 90:
		return GradeAMinus
	case score >= 87:
		return GradeBPlus
	case score >= 83:
		return GradeB
	case score >= 80:
		return GradeBMinus
	case score >= 77:
		return GradeCPlus
	case score >= 73:
		return GradeC
	case score >= 70:
		return GradeCMinus
	case score >= 67:
		return GradeDPlus
	case score >= 63:
		return GradeD
	case score >= 60:
		return GradeDMinus
	default:
		return GradeF
	}
}

// Weights defines the weights for each component in the composite score.
type Weights struct {
	Complexity  float64 `json:"complexity" toml:"complexity"`
	Duplication float64 `json:"duplication" toml:"duplication"`
	Defect      float64 `json:"defect" toml:"defect"`
	Debt        float64 `json:"debt" toml:"debt"`
	Coupling    float64 `json:"coupling" toml:"coupling"`
	Smells      float64 `json:"smells" toml:"smells"`
	Cohesion    float64 `json:"cohesion" toml:"cohesion"` // Optional, for OO-heavy codebases
}

// DefaultWeights returns the default weights (must sum to 1.0).
func DefaultWeights() Weights {
	return Weights{
		Complexity:  0.25,
		Duplication: 0.20,
		Defect:      0.25,
		Debt:        0.15,
		Coupling:    0.10,
		Smells:      0.05,
	}
}

// Thresholds defines minimum acceptable scores for each component.
type Thresholds struct {
	Score       int `json:"score" toml:"score"`
	Complexity  int `json:"complexity" toml:"complexity"`
	Duplication int `json:"duplication" toml:"duplication"`
	Defect      int `json:"defect" toml:"defect"`
	Debt        int `json:"debt" toml:"debt"`
	Coupling    int `json:"coupling" toml:"coupling"`
	Smells      int `json:"smells" toml:"smells"`
	Cohesion    int `json:"cohesion" toml:"cohesion"`
}

// ComponentScores holds the individual component scores (0-100 each).
type ComponentScores struct {
	Complexity  int `json:"complexity"`
	Duplication int `json:"duplication"`
	Defect      int `json:"defect"`
	Debt        int `json:"debt"`
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
	Grade            string                     `json:"grade"`
	Components       ComponentScores            `json:"components"`
	CohesionIncluded bool                       `json:"cohesion_included"` // True if cohesion weight > 0
	Weights          Weights                    `json:"weights"`
	FilesAnalyzed    int                        `json:"files_analyzed"`
	Thresholds       map[string]ThresholdResult `json:"thresholds,omitempty"`
	Passed           bool                       `json:"passed"`
	Timestamp        time.Time                  `json:"timestamp"`
	Commit           string                     `json:"commit,omitempty"`
}

// ComputeComposite calculates the weighted composite score and grade.
func (r *Result) ComputeComposite() {
	weighted := float64(r.Components.Complexity)*r.Weights.Complexity +
		float64(r.Components.Duplication)*r.Weights.Duplication +
		float64(r.Components.Defect)*r.Weights.Defect +
		float64(r.Components.Debt)*r.Weights.Debt +
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
	r.Grade = string(GradeFromScore(r.Score))
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
	check("defect", r.Components.Defect, t.Defect)
	check("debt", r.Components.Debt, t.Debt)
	check("coupling", r.Components.Coupling, t.Coupling)
	check("smells", r.Components.Smells, t.Smells)
	check("cohesion", r.Components.Cohesion, t.Cohesion)
}

// Normalizer functions are in normalize.go with detailed documentation
// explaining the rationale and calibration for each scoring algorithm.
