package analyzer

import (
	"context"
	"regexp"
	"strings"
	"time"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/models"
)

// ChangesAnalyzer implements change-level defect prediction based on
// Kamei et al. (2013) "A Large-Scale Empirical Study of Just-in-Time Quality Assurance".
type ChangesAnalyzer struct {
	days      int
	weights   models.ChangesWeights
	opener    vcs.Opener
	reference time.Time // Reference time for analysis (defaults to time.Now())
}

// ChangesOption is a functional option for configuring ChangesAnalyzer.
type ChangesOption func(*ChangesAnalyzer)

// WithChangesDays sets the number of days of git history to analyze.
func WithChangesDays(days int) ChangesOption {
	return func(a *ChangesAnalyzer) {
		if days > 0 {
			a.days = days
		}
	}
}

// WithChangesWeights sets custom weights for change risk prediction.
func WithChangesWeights(weights models.ChangesWeights) ChangesOption {
	return func(a *ChangesAnalyzer) {
		a.weights = weights
	}
}

// WithChangesOpener sets the VCS opener (useful for testing).
func WithChangesOpener(opener vcs.Opener) ChangesOption {
	return func(a *ChangesAnalyzer) {
		a.opener = opener
	}
}

// WithChangesReferenceTime sets the reference time for analysis.
// This is useful for reproducible tests or historical analysis.
// If not set, defaults to time.Now().
func WithChangesReferenceTime(t time.Time) ChangesOption {
	return func(a *ChangesAnalyzer) {
		a.reference = t
	}
}

// NewChangesAnalyzer creates a new change-level defect prediction analyzer.
func NewChangesAnalyzer(opts ...ChangesOption) *ChangesAnalyzer {
	a := &ChangesAnalyzer{
		days:    30,
		weights: models.DefaultChangesWeights(),
		opener:  vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// Bug fix regex patterns - detect commits that fix bugs.
// Based on patterns from Mockus & Votta (2000) and subsequent research.
var bugFixPatterns = []*regexp.Regexp{
	regexp.MustCompile(`(?i)\bfix(es|ed|ing)?\b`),
	regexp.MustCompile(`(?i)\bbug\b`),
	regexp.MustCompile(`(?i)\bbugfix\b`),
	regexp.MustCompile(`(?i)\bpatch(es|ed|ing)?\b`),
	regexp.MustCompile(`(?i)\bresolve[sd]?\b`),
	regexp.MustCompile(`(?i)\bclose[sd]?\s+#\d+`),
	regexp.MustCompile(`(?i)\bfixes?\s+#\d+`),
	regexp.MustCompile(`(?i)\bdefect\b`),
	regexp.MustCompile(`(?i)\bissue\b`),
	regexp.MustCompile(`(?i)\berror\b`),
	regexp.MustCompile(`(?i)\bcrash(es|ed|ing)?\b`),
}

// Automated/trivial commit patterns - these are low-risk by nature.
var automatedCommitPatterns = []*regexp.Regexp{
	regexp.MustCompile(`(?i)^\s*chore:\s*updated?\s+(image\s+)?tag`),
	regexp.MustCompile(`(?i)\[skip ci\]`),
	regexp.MustCompile(`(?i)^\s*Merge\s+(pull\s+request|branch)`),
	regexp.MustCompile(`(?i)^\s*chore\(deps\):`),
	regexp.MustCompile(`(?i)^\s*chore:\s*bump\s+version`),
	regexp.MustCompile(`(?i)^\s*ci:`),
	regexp.MustCompile(`(?i)^\s*docs?:`),
	regexp.MustCompile(`(?i)^\s*style:`),
}

// isBugFixCommit checks if a commit message indicates a bug fix.
func isBugFixCommit(message string) bool {
	for _, pattern := range bugFixPatterns {
		if pattern.MatchString(message) {
			return true
		}
	}
	return false
}

// isAutomatedCommit checks if a commit is automated/trivial.
func isAutomatedCommit(message string) bool {
	for _, pattern := range automatedCommitPatterns {
		if pattern.MatchString(message) {
			return true
		}
	}
	return false
}

// AnalyzeRepo performs change-level defect prediction on repository commits.
func (a *ChangesAnalyzer) AnalyzeRepo(repoPath string) (*models.ChangesAnalysis, error) {
	ctx, cancel := context.WithTimeout(context.Background(), DefaultGitTimeout)
	defer cancel()
	return a.AnalyzeRepoWithContext(ctx, repoPath)
}

// AnalyzeRepoWithContext performs change analysis with context for cancellation/timeout.
func (a *ChangesAnalyzer) AnalyzeRepoWithContext(ctx context.Context, repoPath string) (*models.ChangesAnalysis, error) {
	repo, err := a.opener.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	refTime := a.reference
	if refTime.IsZero() {
		refTime = time.Now()
	}
	cutoff := refTime.AddDate(0, 0, -a.days)

	logIter, err := repo.Log(&vcs.LogOptions{
		Since: &cutoff,
	})
	if err != nil {
		return nil, err
	}
	defer logIter.Close()

	// First pass: collect raw commit data (git log returns newest-first)
	rawCommits, err := a.collectCommitData(ctx, logIter)
	if err != nil {
		return nil, err
	}

	// Reverse to process oldest-first for correct state-dependent metrics
	reverseCommits(rawCommits)

	// Second pass: compute state-dependent features in chronological order
	commits := computeStateDependentFeatures(rawCommits)

	// Build and return analysis result
	return a.buildChangesAnalysis(commits), nil
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

// boolToFloat converts bool to float64.
func boolToFloat(b bool) float64 {
	if b {
		return 1.0
	}
	return 0.0
}

// truncateMessage truncates commit message to first line or 80 chars.
func truncateMessage(message string) string {
	if idx := strings.Index(message, "\n"); idx > 0 {
		message = message[:idx]
	}
	if len(message) > 80 {
		message = message[:77] + "..."
	}
	return strings.TrimSpace(message)
}

// calculateNormalizationStats computes min-max values for normalization.
func calculateNormalizationStats(commits []models.CommitFeatures) models.NormalizationStats {
	stats := models.NormalizationStats{}

	for _, c := range commits {
		if c.LinesAdded > stats.MaxLinesAdded {
			stats.MaxLinesAdded = c.LinesAdded
		}
		if c.LinesDeleted > stats.MaxLinesDeleted {
			stats.MaxLinesDeleted = c.LinesDeleted
		}
		if c.NumFiles > stats.MaxNumFiles {
			stats.MaxNumFiles = c.NumFiles
		}
		if c.UniqueChanges > stats.MaxUniqueChanges {
			stats.MaxUniqueChanges = c.UniqueChanges
		}
		if c.NumDevelopers > stats.MaxNumDevelopers {
			stats.MaxNumDevelopers = c.NumDevelopers
		}
		if c.AuthorExperience > stats.MaxAuthorExperience {
			stats.MaxAuthorExperience = c.AuthorExperience
		}
		if c.Entropy > stats.MaxEntropy {
			stats.MaxEntropy = c.Entropy
		}
	}

	// Ensure no zero max values to avoid division by zero
	if stats.MaxLinesAdded == 0 {
		stats.MaxLinesAdded = 1
	}
	if stats.MaxLinesDeleted == 0 {
		stats.MaxLinesDeleted = 1
	}
	if stats.MaxNumFiles == 0 {
		stats.MaxNumFiles = 1
	}
	if stats.MaxUniqueChanges == 0 {
		stats.MaxUniqueChanges = 1
	}
	if stats.MaxNumDevelopers == 0 {
		stats.MaxNumDevelopers = 1
	}
	if stats.MaxAuthorExperience == 0 {
		stats.MaxAuthorExperience = 1
	}
	if stats.MaxEntropy == 0 {
		stats.MaxEntropy = 1
	}

	return stats
}

// changesPercentile calculates the p-th percentile of a sorted slice.
func changesPercentile(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// Close releases analyzer resources.
func (a *ChangesAnalyzer) Close() {
	// No resources to release
}
