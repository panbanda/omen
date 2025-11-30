package analyzer

import (
	"context"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/models"
)

// JITAnalyzer implements Just-in-Time defect prediction based on
// Kamei et al. (2013) "A Large-Scale Empirical Study of Just-in-Time Quality Assurance".
type JITAnalyzer struct {
	days      int
	weights   models.JITWeights
	opener    vcs.Opener
	reference time.Time // Reference time for analysis (defaults to time.Now())
}

// JITOption is a functional option for configuring JITAnalyzer.
type JITOption func(*JITAnalyzer)

// WithJITDays sets the number of days of git history to analyze.
func WithJITDays(days int) JITOption {
	return func(a *JITAnalyzer) {
		if days > 0 {
			a.days = days
		}
	}
}

// WithJITWeights sets custom weights for JIT prediction.
func WithJITWeights(weights models.JITWeights) JITOption {
	return func(a *JITAnalyzer) {
		a.weights = weights
	}
}

// WithJITOpener sets the VCS opener (useful for testing).
func WithJITOpener(opener vcs.Opener) JITOption {
	return func(a *JITAnalyzer) {
		a.opener = opener
	}
}

// WithJITReferenceTime sets the reference time for analysis.
// This is useful for reproducible tests or historical analysis.
// If not set, defaults to time.Now().
func WithJITReferenceTime(t time.Time) JITOption {
	return func(a *JITAnalyzer) {
		a.reference = t
	}
}

// NewJITAnalyzer creates a new JIT defect prediction analyzer.
func NewJITAnalyzer(opts ...JITOption) *JITAnalyzer {
	a := &JITAnalyzer{
		days:    30,
		weights: models.DefaultJITWeights(),
		opener:  vcs.DefaultOpener(),
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// Bug fix regex patterns
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

// Automated/trivial commit patterns - these are low-risk by nature
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

// AnalyzeRepo performs JIT defect prediction on repository commits.
func (a *JITAnalyzer) AnalyzeRepo(repoPath string) (*models.JITAnalysis, error) {
	ctx, cancel := context.WithTimeout(context.Background(), DefaultGitTimeout)
	defer cancel()
	return a.AnalyzeRepoWithContext(ctx, repoPath)
}

// AnalyzeRepoWithContext performs JIT analysis with context for cancellation/timeout.
func (a *JITAnalyzer) AnalyzeRepoWithContext(ctx context.Context, repoPath string) (*models.JITAnalysis, error) {
	repo, err := a.opener.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	// Use configured reference time or default to now
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

	// Track author experience and file history
	authorCommits := make(map[string]int)           // author -> total commits
	fileChanges := make(map[string]int)             // file -> total commits
	fileAuthors := make(map[string]map[string]bool) // file -> set of authors

	// First pass: collect features from commits
	var commits []models.CommitFeatures

	err = logIter.ForEach(func(commit vcs.Commit) error {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if commit.NumParents() == 0 {
			return nil // Skip initial commit
		}

		author := commit.Author().Name
		message := commit.Message()

		// Get parent for diff
		parent, err := commit.Parent(0)
		if err != nil {
			return nil
		}

		parentTree, err := parent.Tree()
		if err != nil {
			return nil
		}

		commitTree, err := commit.Tree()
		if err != nil {
			return nil
		}

		changes, err := parentTree.Diff(commitTree)
		if err != nil {
			return nil
		}

		// Extract features
		features := models.CommitFeatures{
			CommitHash:       commit.Hash().String(),
			Author:           author,
			Message:          truncateMessage(message),
			Timestamp:        commit.Author().When,
			IsFix:            isBugFixCommit(message),
			IsAutomated:      isAutomatedCommit(message),
			FilesModified:    make([]string, 0),
			AuthorExperience: authorCommits[author],
		}

		linesPerFile := make(map[string]int)
		uniqueDevs := make(map[string]bool)
		priorCommits := 0

		for _, change := range changes {
			filePath := change.ToName()
			if filePath == "" {
				filePath = change.FromName() // Deleted file
			}

			features.FilesModified = append(features.FilesModified, filePath)

			// Count lines changed
			patch, err := change.Patch()
			if err == nil {
				for _, filePatch := range patch.FilePatches() {
					for _, chunk := range filePatch.Chunks() {
						content := chunk.Content()
						lines := strings.Count(content, "\n")
						switch chunk.Type() {
						case vcs.ChunkAdd:
							features.LinesAdded += lines
							linesPerFile[filePath] += lines
						case vcs.ChunkDelete:
							features.LinesDeleted += lines
							linesPerFile[filePath] += lines
						}
					}
				}
			}

			// Track unique changes (prior commits to this file)
			priorCommits += fileChanges[filePath]

			// Track unique developers
			if authors, ok := fileAuthors[filePath]; ok {
				for auth := range authors {
					uniqueDevs[auth] = true
				}
			}
		}

		features.NumFiles = len(features.FilesModified)
		features.UniqueChanges = priorCommits
		features.NumDevelopers = len(uniqueDevs)
		features.Entropy = models.CalculateEntropy(linesPerFile)

		commits = append(commits, features)

		// Update tracking for future commits
		authorCommits[author]++
		for _, file := range features.FilesModified {
			fileChanges[file]++
			if fileAuthors[file] == nil {
				fileAuthors[file] = make(map[string]bool)
			}
			fileAuthors[file][author] = true
		}

		return nil
	})

	if err != nil {
		return nil, err
	}

	// Build analysis result
	analysis := models.NewJITAnalysis()
	analysis.PeriodDays = a.days
	analysis.Weights = a.weights

	if len(commits) == 0 {
		return analysis, nil
	}

	// Calculate normalization stats (min-max scaling)
	analysis.Normalization = calculateNormalizationStats(commits)

	// Second pass: calculate risk scores
	var totalScore float64
	scores := make([]float64, 0, len(commits))

	for _, features := range commits {
		score := models.CalculateJITRisk(features, a.weights, analysis.Normalization)
		level := models.GetJITRiskLevel(score)

		factors := map[string]float64{
			"fix":            boolToFloat(features.IsFix) * a.weights.FIX,
			"entropy":        safeNormalize(features.Entropy, analysis.Normalization.MaxEntropy) * a.weights.Entropy,
			"lines_added":    safeNormalizeInt(features.LinesAdded, analysis.Normalization.MaxLinesAdded) * a.weights.LA,
			"lines_deleted":  safeNormalizeInt(features.LinesDeleted, analysis.Normalization.MaxLinesDeleted) * a.weights.LD,
			"num_files":      safeNormalizeInt(features.NumFiles, analysis.Normalization.MaxNumFiles) * a.weights.NF,
			"unique_changes": safeNormalizeInt(features.UniqueChanges, analysis.Normalization.MaxUniqueChanges) * a.weights.NUC,
			"num_developers": safeNormalizeInt(features.NumDevelopers, analysis.Normalization.MaxNumDevelopers) * a.weights.NDEV,
			"experience":     (1.0 - safeNormalizeInt(features.AuthorExperience, analysis.Normalization.MaxAuthorExperience)) * a.weights.EXP,
		}

		risk := models.CommitRisk{
			CommitHash:          features.CommitHash,
			Author:              features.Author,
			Message:             features.Message,
			Timestamp:           features.Timestamp,
			RiskScore:           score,
			RiskLevel:           level,
			ContributingFactors: factors,
			Recommendations:     models.GenerateJITRecommendations(features, score, factors),
			FilesModified:       features.FilesModified,
		}

		analysis.Commits = append(analysis.Commits, risk)
		totalScore += score
		scores = append(scores, score)

		// Update summary counts
		switch level {
		case models.JITRiskHigh:
			analysis.Summary.HighRiskCount++
		case models.JITRiskMedium:
			analysis.Summary.MediumRiskCount++
		case models.JITRiskLow:
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
	if len(commits) > 0 {
		analysis.Summary.AvgRiskScore = totalScore / float64(len(commits))

		sort.Float64s(scores)
		analysis.Summary.P50RiskScore = jitPercentile(scores, 50)
		analysis.Summary.P95RiskScore = jitPercentile(scores, 95)
	}

	return analysis, nil
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

	// Ensure no zero max values
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

// jitPercentile calculates the p-th percentile of a sorted slice.
func jitPercentile(sorted []float64, p int) float64 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
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
	// Get first line
	if idx := strings.Index(message, "\n"); idx > 0 {
		message = message[:idx]
	}
	// Truncate if too long
	if len(message) > 80 {
		message = message[:77] + "..."
	}
	return strings.TrimSpace(message)
}

// Close releases analyzer resources.
func (a *JITAnalyzer) Close() {
	// No resources to release
}
