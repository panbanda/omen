package analyzer

import (
	"sort"

	"github.com/panbanda/omen/pkg/models"
)

// buildChangesAnalysis constructs the analysis result from processed commits.
func (a *ChangesAnalyzer) buildChangesAnalysis(commits []models.CommitFeatures) *models.ChangesAnalysis {
	analysis := models.NewChangesAnalysis()
	analysis.PeriodDays = a.days
	analysis.Weights = a.weights

	if len(commits) == 0 {
		return analysis
	}

	analysis.Normalization = calculateNormalizationStats(commits)

	var totalScore float64
	scores := make([]float64, 0, len(commits))

	for _, features := range commits {
		risk := a.calculateCommitRisk(features, analysis.Normalization)
		analysis.Commits = append(analysis.Commits, risk)
		totalScore += risk.RiskScore
		scores = append(scores, risk.RiskScore)

		switch risk.RiskLevel {
		case models.ChangeRiskHigh:
			analysis.Summary.HighRiskCount++
		case models.ChangeRiskMedium:
			analysis.Summary.MediumRiskCount++
		case models.ChangeRiskLow:
			analysis.Summary.LowRiskCount++
		}

		if features.IsFix {
			analysis.Summary.BugFixCount++
		}
	}

	// Sort by risk score descending
	sort.Slice(analysis.Commits, func(i, j int) bool {
		return analysis.Commits[i].RiskScore > analysis.Commits[j].RiskScore
	})

	// Calculate summary statistics
	analysis.Summary.TotalCommits = len(commits)
	analysis.Summary.AvgRiskScore = totalScore / float64(len(commits))

	sort.Float64s(scores)
	analysis.Summary.P50RiskScore = changesPercentile(scores, 50)
	analysis.Summary.P95RiskScore = changesPercentile(scores, 95)

	return analysis
}

// calculateCommitRisk computes risk score and contributing factors for a single commit.
func (a *ChangesAnalyzer) calculateCommitRisk(features models.CommitFeatures, norm models.NormalizationStats) models.CommitRisk {
	score := models.CalculateChangeRisk(features, a.weights, norm)
	level := models.GetChangeRiskLevel(score)

	factors := map[string]float64{
		"fix":            boolToFloat(features.IsFix) * a.weights.FIX,
		"entropy":        safeNormalize(features.Entropy, norm.MaxEntropy) * a.weights.Entropy,
		"lines_added":    safeNormalizeInt(features.LinesAdded, norm.MaxLinesAdded) * a.weights.LA,
		"lines_deleted":  safeNormalizeInt(features.LinesDeleted, norm.MaxLinesDeleted) * a.weights.LD,
		"num_files":      safeNormalizeInt(features.NumFiles, norm.MaxNumFiles) * a.weights.NF,
		"unique_changes": safeNormalizeInt(features.UniqueChanges, norm.MaxUniqueChanges) * a.weights.NUC,
		"num_developers": safeNormalizeInt(features.NumDevelopers, norm.MaxNumDevelopers) * a.weights.NDEV,
		"experience":     (1.0 - safeNormalizeInt(features.AuthorExperience, norm.MaxAuthorExperience)) * a.weights.EXP,
	}

	return models.CommitRisk{
		CommitHash:          features.CommitHash,
		Author:              features.Author,
		Message:             features.Message,
		Timestamp:           features.Timestamp,
		RiskScore:           score,
		RiskLevel:           level,
		ContributingFactors: factors,
		Recommendations:     models.GenerateChangeRecommendations(features, score, factors),
		FilesModified:       features.FilesModified,
	}
}
