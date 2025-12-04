package changes

import (
	"context"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/stats"
)

// DefaultGitTimeout is the default timeout for git operations.
const DefaultGitTimeout = 5 * time.Minute

// Analyzer implements change-level defect prediction based on
// Kamei et al. (2013) "A Large-Scale Empirical Study of Just-in-Time Quality Assurance".
type Analyzer struct {
	days      int
	weights   Weights
	opener    vcs.Opener
	reference time.Time // Reference time for analysis (defaults to time.Now())
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithDays sets the number of days of git history to analyze.
func WithDays(days int) Option {
	return func(a *Analyzer) {
		if days > 0 {
			a.days = days
		}
	}
}

// WithWeights sets custom weights for change risk prediction.
func WithWeights(weights Weights) Option {
	return func(a *Analyzer) {
		a.weights = weights
	}
}

// WithOpener sets the VCS opener (useful for testing).
func WithOpener(opener vcs.Opener) Option {
	return func(a *Analyzer) {
		a.opener = opener
	}
}

// WithReferenceTime sets the reference time for analysis.
// This is useful for reproducible tests or historical analysis.
// If not set, defaults to time.Now().
func WithReferenceTime(t time.Time) Option {
	return func(a *Analyzer) {
		a.reference = t
	}
}

// New creates a new change-level defect prediction analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		days:    30,
		weights: DefaultWeights(),
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
func (a *Analyzer) AnalyzeRepo(repoPath string) (*Analysis, error) {
	ctx, cancel := context.WithTimeout(context.Background(), DefaultGitTimeout)
	defer cancel()
	return a.AnalyzeRepoWithContext(ctx, repoPath)
}

// AnalyzeRepoWithContext performs change analysis with context for cancellation/timeout.
func (a *Analyzer) AnalyzeRepoWithContext(ctx context.Context, repoPath string) (*Analysis, error) {
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
	return a.buildAnalysis(commits), nil
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
func calculateNormalizationStats(commits []CommitFeatures) NormalizationStats {
	stats := NormalizationStats{}

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

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	// No resources to release
}

// rawCommit holds commit data collected in the first pass.
// State-dependent fields (AuthorExperience, NumDevelopers, UniqueChanges)
// are computed in the second pass after sorting by timestamp.
type rawCommit struct {
	features     CommitFeatures
	linesPerFile map[string]int // for entropy calculation
}

// collectCommitData iterates through commits and extracts raw commit data.
// Returns commits in git log order (newest-first).
func (a *Analyzer) collectCommitData(ctx context.Context, logIter vcs.CommitIterator) ([]rawCommit, error) {
	var rawCommits []rawCommit

	err := logIter.ForEach(func(commit vcs.Commit) error {
		select {
		case <-ctx.Done():
			return ctx.Err()
		default:
		}

		if commit.NumParents() == 0 {
			return nil // Skip initial commit
		}

		raw, err := a.extractCommitFeatures(commit)
		if err != nil {
			return nil // Skip commits that can't be processed
		}

		rawCommits = append(rawCommits, raw)
		return nil
	})

	return rawCommits, err
}

// extractCommitFeatures extracts commit-local features (no state dependencies).
func (a *Analyzer) extractCommitFeatures(commit vcs.Commit) (rawCommit, error) {
	author := commit.Author().Name
	message := commit.Message()

	parent, err := commit.Parent(0)
	if err != nil {
		return rawCommit{}, err
	}

	parentTree, err := parent.Tree()
	if err != nil {
		return rawCommit{}, err
	}

	commitTree, err := commit.Tree()
	if err != nil {
		return rawCommit{}, err
	}

	changes, err := parentTree.Diff(commitTree)
	if err != nil {
		return rawCommit{}, err
	}

	features := CommitFeatures{
		CommitHash:    commit.Hash().String(),
		Author:        author,
		Message:       truncateMessage(message),
		Timestamp:     commit.Author().When,
		IsFix:         isBugFixCommit(message),
		IsAutomated:   isAutomatedCommit(message),
		FilesModified: make([]string, 0),
	}

	linesPerFile := make(map[string]int)

	for _, change := range changes {
		filePath := change.ToName()
		if filePath == "" {
			filePath = change.FromName() // Deleted file
		}

		features.FilesModified = append(features.FilesModified, filePath)

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
	}

	features.NumFiles = len(features.FilesModified)
	features.Entropy = CalculateEntropy(linesPerFile)

	return rawCommit{
		features:     features,
		linesPerFile: linesPerFile,
	}, nil
}

// reverseCommits reverses the commit slice in place for chronological processing.
// Git log returns newest-first; we need oldest-first for state-dependent metrics.
func reverseCommits(commits []rawCommit) {
	for i, j := 0, len(commits)-1; i < j; i, j = i+1, j-1 {
		commits[i], commits[j] = commits[j], commits[i]
	}
}

// computeStateDependentFeatures computes features that depend on historical state.
// Processes commits chronologically (oldest-first) and tracks author experience,
// file change history, and developer counts.
func computeStateDependentFeatures(rawCommits []rawCommit) []CommitFeatures {
	// State tracked across commits
	authorCommits := make(map[string]int)           // author -> commits made BEFORE current
	fileChanges := make(map[string]int)             // file -> commits touching it BEFORE current
	fileAuthors := make(map[string]map[string]bool) // file -> authors who touched it BEFORE current

	var commits []CommitFeatures
	for _, raw := range rawCommits {
		features := raw.features
		author := features.Author

		// Look up state BEFORE this commit
		features.AuthorExperience = authorCommits[author]

		// Calculate NumDevelopers and UniqueChanges from state BEFORE this commit
		uniqueDevs := make(map[string]bool)
		priorCommits := 0
		for _, filePath := range features.FilesModified {
			priorCommits += fileChanges[filePath]
			if authors, ok := fileAuthors[filePath]; ok {
				for auth := range authors {
					uniqueDevs[auth] = true
				}
			}
		}
		features.NumDevelopers = len(uniqueDevs)
		features.UniqueChanges = priorCommits

		commits = append(commits, features)

		// Update state AFTER processing this commit (for future commits)
		authorCommits[author]++
		for _, file := range features.FilesModified {
			fileChanges[file]++
			if fileAuthors[file] == nil {
				fileAuthors[file] = make(map[string]bool)
			}
			fileAuthors[file][author] = true
		}
	}

	return commits
}

// buildAnalysis constructs the analysis result from processed commits.
func (a *Analyzer) buildAnalysis(commits []CommitFeatures) *Analysis {
	analysis := NewAnalysis()
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
		case RiskLevelHigh:
			analysis.Summary.HighRiskCount++
		case RiskLevelMedium:
			analysis.Summary.MediumRiskCount++
		case RiskLevelLow:
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
	analysis.Summary.P50RiskScore = stats.Percentile(scores, 50)
	analysis.Summary.P95RiskScore = stats.Percentile(scores, 95)

	return analysis
}

// calculateCommitRisk computes risk score and contributing factors for a single commit.
func (a *Analyzer) calculateCommitRisk(features CommitFeatures, norm NormalizationStats) CommitRisk {
	score := CalculateRisk(features, a.weights, norm)
	level := GetRiskLevel(score)

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

	return CommitRisk{
		CommitHash:          features.CommitHash,
		Author:              features.Author,
		Message:             features.Message,
		Timestamp:           features.Timestamp,
		RiskScore:           score,
		RiskLevel:           level,
		ContributingFactors: factors,
		Recommendations:     GenerateRecommendations(features, score, factors),
		FilesModified:       features.FilesModified,
	}
}
