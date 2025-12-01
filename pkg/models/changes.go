package models

import (
	"math"
	"time"
)

// ChangesWeights defines the weights for change-level defect prediction features.
// Based on Kamei et al. (2013) "A Large-Scale Empirical Study of Just-in-Time Quality Assurance"
// and Zeng et al. (2021) showing simple models match deep learning accuracy (~65%).
type ChangesWeights struct {
	FIX     float64 `json:"fix"`     // Is bug fix commit?
	Entropy float64 `json:"entropy"` // Change entropy across files
	LA      float64 `json:"la"`      // Lines added
	NUC     float64 `json:"nuc"`     // Number of unique prior commits
	NF      float64 `json:"nf"`      // Number of files modified
	LD      float64 `json:"ld"`      // Lines deleted
	NDEV    float64 `json:"ndev"`    // Number of developers
	EXP     float64 `json:"exp"`     // Author experience (inverted)
}

// DefaultChangesWeights returns research-backed weights from the requirements spec.
func DefaultChangesWeights() ChangesWeights {
	return ChangesWeights{
		FIX:     0.25,
		Entropy: 0.20,
		LA:      0.20,
		NUC:     0.10,
		NF:      0.10,
		LD:      0.05,
		NDEV:    0.05,
		EXP:     0.05,
	}
}

// CommitFeatures represents features extracted from a commit for change risk analysis.
type CommitFeatures struct {
	CommitHash       string    `json:"commit_hash"`
	Author           string    `json:"author"`
	Message          string    `json:"message"`
	Timestamp        time.Time `json:"timestamp"`
	IsFix            bool      `json:"is_fix"`            // FIX: Bug fix commit?
	IsAutomated      bool      `json:"is_automated"`      // Automated/trivial commit (CI, merge, etc.)
	Entropy          float64   `json:"entropy"`           // Entropy: Change distribution
	LinesAdded       int       `json:"lines_added"`       // LA
	LinesDeleted     int       `json:"lines_deleted"`     // LD
	NumFiles         int       `json:"num_files"`         // NF
	UniqueChanges    int       `json:"unique_changes"`    // NUC: Prior commits to these files
	NumDevelopers    int       `json:"num_developers"`    // NDEV: Unique devs on these files
	AuthorExperience int       `json:"author_experience"` // EXP: Author's prior commits
	FilesModified    []string  `json:"files_modified"`
}

// ChangeRiskLevel represents the risk level for a commit.
type ChangeRiskLevel string

const (
	ChangeRiskLow    ChangeRiskLevel = "low"    // < 0.4
	ChangeRiskMedium ChangeRiskLevel = "medium" // 0.4 - 0.7
	ChangeRiskHigh   ChangeRiskLevel = "high"   // >= 0.7
)

// CommitRisk represents the change risk prediction result for a single commit.
type CommitRisk struct {
	CommitHash          string             `json:"commit_hash"`
	Author              string             `json:"author"`
	Message             string             `json:"message"`
	Timestamp           time.Time          `json:"timestamp"`
	RiskScore           float64            `json:"risk_score"`
	RiskLevel           ChangeRiskLevel    `json:"risk_level"`
	ContributingFactors map[string]float64 `json:"contributing_factors"`
	Recommendations     []string           `json:"recommendations"`
	FilesModified       []string           `json:"files_modified"`
}

// ChangesAnalysis represents the full change-level defect prediction result.
type ChangesAnalysis struct {
	GeneratedAt   time.Time          `json:"generated_at"`
	PeriodDays    int                `json:"period_days"`
	Commits       []CommitRisk       `json:"commits"`
	Summary       ChangesSummary     `json:"summary"`
	Weights       ChangesWeights     `json:"weights"`
	Normalization NormalizationStats `json:"normalization"`
}

// ChangesSummary provides aggregate statistics.
type ChangesSummary struct {
	TotalCommits    int     `json:"total_commits"`
	HighRiskCount   int     `json:"high_risk_count"`
	MediumRiskCount int     `json:"medium_risk_count"`
	LowRiskCount    int     `json:"low_risk_count"`
	BugFixCount     int     `json:"bug_fix_count"`
	AvgRiskScore    float64 `json:"avg_risk_score"`
	P50RiskScore    float64 `json:"p50_risk_score"`
	P95RiskScore    float64 `json:"p95_risk_score"`
}

// NormalizationStats holds min-max values for normalization.
type NormalizationStats struct {
	MaxLinesAdded       int     `json:"max_lines_added"`
	MaxLinesDeleted     int     `json:"max_lines_deleted"`
	MaxNumFiles         int     `json:"max_num_files"`
	MaxUniqueChanges    int     `json:"max_unique_changes"`
	MaxNumDevelopers    int     `json:"max_num_developers"`
	MaxAuthorExperience int     `json:"max_author_experience"`
	MaxEntropy          float64 `json:"max_entropy"`
}

// NewChangesAnalysis creates an initialized changes analysis.
func NewChangesAnalysis() *ChangesAnalysis {
	return &ChangesAnalysis{
		GeneratedAt: time.Now().UTC(),
		Commits:     make([]CommitRisk, 0),
		Weights:     DefaultChangesWeights(),
	}
}

// CalculateChangeRisk computes the risk score for a commit using change features.
func CalculateChangeRisk(features CommitFeatures, weights ChangesWeights, norm NormalizationStats) float64 {
	// Automated commits (CI, merges, docs, style) are inherently low risk
	if features.IsAutomated {
		return 0.05 // Minimal risk score for automated commits
	}

	// Normalize each feature using min-max scaling
	fixNorm := 0.0
	if features.IsFix {
		fixNorm = 1.0
	}

	entropyNorm := safeNormalize(features.Entropy, norm.MaxEntropy)
	laNorm := safeNormalizeInt(features.LinesAdded, norm.MaxLinesAdded)
	ldNorm := safeNormalizeInt(features.LinesDeleted, norm.MaxLinesDeleted)
	nfNorm := safeNormalizeInt(features.NumFiles, norm.MaxNumFiles)
	nucNorm := safeNormalizeInt(features.UniqueChanges, norm.MaxUniqueChanges)
	ndevNorm := safeNormalizeInt(features.NumDevelopers, norm.MaxNumDevelopers)

	// Experience is inverted: less experience = more risk
	expNorm := 1.0 - safeNormalizeInt(features.AuthorExperience, norm.MaxAuthorExperience)

	// Calculate weighted sum
	score := weights.FIX*fixNorm +
		weights.Entropy*entropyNorm +
		weights.LA*laNorm +
		weights.LD*ldNorm +
		weights.NF*nfNorm +
		weights.NUC*nucNorm +
		weights.NDEV*ndevNorm +
		weights.EXP*expNorm

	// Clamp to [0, 1]
	if score < 0 {
		return 0
	}
	if score > 1 {
		return 1
	}
	return score
}

// safeNormalize performs min-max normalization with zero max handling.
func safeNormalize(value, max float64) float64 {
	if max <= 0 {
		return 0
	}
	if value >= max {
		return 1
	}
	return value / max
}

// safeNormalizeInt performs min-max normalization for integers.
func safeNormalizeInt(value, max int) float64 {
	if max <= 0 {
		return 0
	}
	if value >= max {
		return 1
	}
	return float64(value) / float64(max)
}

// CalculateEntropy computes Shannon entropy of changes across files.
// Entropy = -sum(p_i * log2(p_i)) where p_i = lines_in_file_i / total_lines
func CalculateEntropy(linesPerFile map[string]int) float64 {
	if len(linesPerFile) == 0 {
		return 0
	}

	total := 0
	for _, lines := range linesPerFile {
		total += lines
	}

	if total == 0 {
		return 0
	}

	entropy := 0.0
	for _, lines := range linesPerFile {
		if lines > 0 {
			p := float64(lines) / float64(total)
			entropy -= p * math.Log2(p)
		}
	}

	return entropy
}

// GetChangeRiskLevel determines risk level from score.
func GetChangeRiskLevel(score float64) ChangeRiskLevel {
	switch {
	case score >= 0.7:
		return ChangeRiskHigh
	case score >= 0.4:
		return ChangeRiskMedium
	default:
		return ChangeRiskLow
	}
}

// GenerateChangeRecommendations suggests actions based on risk factors.
func GenerateChangeRecommendations(features CommitFeatures, score float64, factors map[string]float64) []string {
	var recs []string

	if features.IsFix {
		recs = append(recs, "Bug fix commit - ensure comprehensive testing of the fix")
	}

	if factors["entropy"] > 0.15 {
		recs = append(recs, "High change entropy - review each modified file carefully")
	}

	if factors["lines_added"] > 0.15 {
		recs = append(recs, "Large addition - consider splitting into smaller commits")
	}

	if factors["num_files"] > 0.08 {
		recs = append(recs, "Many files modified - ensure changes are logically related")
	}

	if factors["experience"] > 0.04 {
		recs = append(recs, "Author has limited experience with these files - request senior review")
	}

	if score >= 0.7 {
		recs = append(recs, "HIGH RISK: Prioritize code review and add comprehensive tests")
	} else if score >= 0.5 {
		recs = append(recs, "Elevated risk: Consider additional testing before merge")
	}

	if len(recs) == 0 {
		recs = append(recs, "Low risk commit - standard review process recommended")
	}

	return recs
}
