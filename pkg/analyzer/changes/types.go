package changes

import (
	"math"
	"time"
)

// Weights defines the weights for change-level defect prediction features.
// Based on Kamei et al. (2013) "A Large-Scale Empirical Study of Just-in-Time Quality Assurance"
// and Zeng et al. (2021) showing simple models match deep learning accuracy (~65%).
type Weights struct {
	FIX     float64 `json:"fix"`     // Is bug fix commit?
	Entropy float64 `json:"entropy"` // Change entropy across files
	LA      float64 `json:"la"`      // Lines added
	NUC     float64 `json:"nuc"`     // Number of unique prior commits
	NF      float64 `json:"nf"`      // Number of files modified
	LD      float64 `json:"ld"`      // Lines deleted
	NDEV    float64 `json:"ndev"`    // Number of developers
	EXP     float64 `json:"exp"`     // Author experience (inverted)
}

// DefaultWeights returns research-backed weights from the requirements spec.
func DefaultWeights() Weights {
	return Weights{
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

// RiskLevel represents the risk level for a commit.
type RiskLevel string

// Risk levels are assigned using percentile-based thresholds following
// JIT (Just-in-Time) defect prediction research best practices.
// Rather than fixed thresholds, commits are ranked relative to the repository:
//   - High:   Top 5% of commits (P95+) - deserve extra scrutiny
//   - Medium: Top 5-20% of commits (P80-P95) - worth additional attention
//   - Low:    Bottom 80% of commits - standard review process
//
// This approach aligns with the 80/20 rule from defect prediction research:
// ~20% of code changes contain ~80% of defects.
// See: Kamei et al. (2013) "A Large-Scale Empirical Study of Just-in-Time Quality Assurance"
const (
	RiskLevelLow    RiskLevel = "low"
	RiskLevelMedium RiskLevel = "medium"
	RiskLevelHigh   RiskLevel = "high"
)

// Percentile thresholds for risk level classification.
// These follow effort-aware JIT defect prediction best practices.
const (
	HighRiskPercentile   = 95 // Top 5% are high risk
	MediumRiskPercentile = 80 // Top 20% (P80-P95) are medium risk
)

// CommitRisk represents the change risk prediction result for a single commit.
type CommitRisk struct {
	CommitHash          string             `json:"commit_hash"`
	Author              string             `json:"author"`
	Message             string             `json:"message"`
	Timestamp           time.Time          `json:"timestamp"`
	RiskScore           float64            `json:"risk_score"`
	RiskLevel           RiskLevel          `json:"risk_level"`
	ContributingFactors map[string]float64 `json:"contributing_factors"`
	Recommendations     []string           `json:"recommendations"`
	FilesModified       []string           `json:"files_modified"`
}

// Analysis represents the full change-level defect prediction result.
type Analysis struct {
	GeneratedAt    time.Time          `json:"generated_at"`
	PeriodDays     int                `json:"period_days"`
	Commits        []CommitRisk       `json:"commits"`
	Summary        Summary            `json:"summary"`
	Weights        Weights            `json:"weights"`
	Normalization  NormalizationStats `json:"normalization"`
	RiskThresholds RiskThresholds     `json:"risk_thresholds"`
}

// Summary provides aggregate statistics.
type Summary struct {
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

// NewAnalysis creates an initialized changes analysis.
func NewAnalysis() *Analysis {
	return &Analysis{
		GeneratedAt: time.Now().UTC(),
		Commits:     make([]CommitRisk, 0),
		Weights:     DefaultWeights(),
	}
}

// CalculateRisk computes the risk score for a commit using change features.
func CalculateRisk(features CommitFeatures, weights Weights, norm NormalizationStats) float64 {
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

// RiskThresholds holds the computed percentile thresholds for risk classification.
type RiskThresholds struct {
	HighThreshold   float64 `json:"high_threshold"`   // Score at P95
	MediumThreshold float64 `json:"medium_threshold"` // Score at P80
}

// DefaultRiskThresholds returns fixed thresholds for single-item analysis
// (e.g., analyzing a PR diff where we don't have a population for percentiles).
// These are based on empirical observations of what constitutes risky changes.
func DefaultRiskThresholds() RiskThresholds {
	return RiskThresholds{
		HighThreshold:   0.6, // Score >= 0.6 indicates significant risk
		MediumThreshold: 0.3, // Score >= 0.3 warrants attention
	}
}

// GetRiskLevel determines risk level from score using percentile-based thresholds.
// The thresholds parameter contains P95 and P80 values computed from all commits.
func GetRiskLevel(score float64, thresholds RiskThresholds) RiskLevel {
	switch {
	case score >= thresholds.HighThreshold:
		return RiskLevelHigh
	case score >= thresholds.MediumThreshold:
		return RiskLevelMedium
	default:
		return RiskLevelLow
	}
}

// GenerateRecommendations suggests actions based on risk factors.
func GenerateRecommendations(features CommitFeatures, score float64, factors map[string]float64) []string {
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
