package changes

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"os/exec"
	"strconv"
	"strings"
	"time"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/panbanda/omen/internal/vcs"
)

// DiffResult represents the risk analysis of a branch diff.
type DiffResult struct {
	GeneratedAt   time.Time          `json:"generated_at"`
	SourceBranch  string             `json:"source_branch"`
	TargetBranch  string             `json:"target_branch"`
	MergeBase     string             `json:"merge_base"`
	Score         float64            `json:"score"`
	Level         string             `json:"level"`
	LinesAdded    int                `json:"lines_added"`
	LinesDeleted  int                `json:"lines_deleted"`
	FilesModified int                `json:"files_modified"`
	Commits       int                `json:"commits"`
	Factors       map[string]float64 `json:"factors"`
}

// AnalyzeDiff analyzes the diff between current branch and target branch.
// If target is empty, auto-detects the default branch (main/master).
func (a *Analyzer) AnalyzeDiff(repoPath string, target string) (*DiffResult, error) {
	ctx, cancel := context.WithTimeout(context.Background(), DefaultGitTimeout)
	defer cancel()
	return a.AnalyzeDiffWithContext(ctx, repoPath, target)
}

// AnalyzeDiffWithContext analyzes the diff with context for cancellation.
func (a *Analyzer) AnalyzeDiffWithContext(ctx context.Context, repoPath string, target string) (*DiffResult, error) {
	repo, err := a.opener.PlainOpen(repoPath)
	if err != nil {
		return nil, err
	}

	// Get current branch name
	sourceBranch, err := getCurrentBranch(repoPath)
	if err != nil {
		return nil, err
	}

	// Auto-detect target if not specified
	if target == "" {
		target, err = detectDefaultBranch(repoPath)
		if err != nil {
			return nil, err
		}
	}

	// Find merge-base
	mergeBase, err := getMergeBase(repoPath, target, "HEAD")
	if err != nil {
		return nil, err
	}

	// Get diff stats
	linesAdded, linesDeleted, filesModified, err := getDiffStats(repoPath, mergeBase)
	if err != nil {
		return nil, err
	}

	// Count commits between merge-base and HEAD
	commitCount, err := getCommitCount(repoPath, mergeBase)
	if err != nil {
		return nil, err
	}

	// Get historical context for normalization using 95th percentile
	// This is robust to outliers while adapting to the repository's patterns
	norm, err := a.getHistoricalNormalization(ctx, repo)
	if err != nil {
		// Fall back to reasonable defaults
		norm = diffNormalization()
	}

	// Calculate entropy from file changes
	linesPerFile, err := getLinesPerFile(repoPath, mergeBase)
	if err != nil {
		linesPerFile = make(map[string]int)
	}
	entropy := CalculateEntropy(linesPerFile)

	// Build features for risk calculation
	features := CommitFeatures{
		LinesAdded:    linesAdded,
		LinesDeleted:  linesDeleted,
		NumFiles:      filesModified,
		Entropy:       entropy,
		UniqueChanges: commitCount,
		IsFix:         false, // Aggregate diff, not a single commit
		IsAutomated:   false,
	}

	// Calculate risk score using fixed thresholds for single-diff analysis
	score := CalculateRisk(features, a.weights, norm)
	level := string(GetRiskLevel(score, DefaultRiskThresholds()))

	// Build contributing factors
	factors := map[string]float64{
		"entropy":       safeNormalize(entropy, norm.MaxEntropy) * a.weights.Entropy,
		"lines_added":   safeNormalizeInt(linesAdded, norm.MaxLinesAdded) * a.weights.LA,
		"lines_deleted": safeNormalizeInt(linesDeleted, norm.MaxLinesDeleted) * a.weights.LD,
		"num_files":     safeNormalizeInt(filesModified, norm.MaxNumFiles) * a.weights.NF,
		"commits":       safeNormalizeInt(commitCount, norm.MaxUniqueChanges) * a.weights.NUC,
	}

	return &DiffResult{
		GeneratedAt:   time.Now().UTC(),
		SourceBranch:  sourceBranch,
		TargetBranch:  target,
		MergeBase:     mergeBase,
		Score:         score,
		Level:         level,
		LinesAdded:    linesAdded,
		LinesDeleted:  linesDeleted,
		FilesModified: filesModified,
		Commits:       commitCount,
		Factors:       factors,
	}, nil
}

// getHistoricalNormalization gets normalization stats from recent history.
func (a *Analyzer) getHistoricalNormalization(ctx context.Context, repo vcs.Repository) (NormalizationStats, error) {
	refTime := a.reference
	if refTime.IsZero() {
		refTime = time.Now()
	}
	cutoff := refTime.AddDate(0, 0, -a.days)

	logIter, err := repo.Log(&vcs.LogOptions{Since: &cutoff})
	if err != nil {
		return NormalizationStats{}, err
	}
	defer logIter.Close()

	rawCommits, err := a.collectCommitData(ctx, logIter)
	if err != nil {
		return NormalizationStats{}, err
	}

	if len(rawCommits) == 0 {
		return defaultNormalization(), nil
	}

	reverseCommits(rawCommits)
	commits := computeStateDependentFeatures(rawCommits)
	return calculateNormalizationStats(commits), nil
}

// defaultNormalization returns reasonable defaults when no history is available.
func defaultNormalization() NormalizationStats {
	return NormalizationStats{
		MaxLinesAdded:       500,
		MaxLinesDeleted:     200,
		MaxNumFiles:         20,
		MaxUniqueChanges:    50,
		MaxNumDevelopers:    5,
		MaxAuthorExperience: 100,
		MaxEntropy:          4.0,
	}
}

// diffNormalization returns fixed thresholds for branch diff analysis.
// These represent sensible PR size limits where exceeding them indicates high risk.
func diffNormalization() NormalizationStats {
	return NormalizationStats{
		MaxLinesAdded:       400, // PRs > 400 lines are hard to review
		MaxLinesDeleted:     200, // Large deletions warrant attention
		MaxNumFiles:         15,  // PRs touching > 15 files are risky
		MaxUniqueChanges:    10,  // > 10 commits suggests scope creep
		MaxNumDevelopers:    3,   // Multiple authors can indicate coordination issues
		MaxAuthorExperience: 100, // Not used for diff analysis
		MaxEntropy:          3.0, // Lower threshold - scattered changes are risky
	}
}

// getCurrentBranch returns the current branch name.
func getCurrentBranch(repoPath string) (string, error) {
	cmd := exec.Command("git", "rev-parse", "--abbrev-ref", "HEAD")
	cmd.Dir = repoPath
	out, err := cmd.Output()
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

// detectDefaultBranch finds main or master branch.
func detectDefaultBranch(repoPath string) (string, error) {
	// Try main first
	cmd := exec.Command("git", "rev-parse", "--verify", "main")
	cmd.Dir = repoPath
	if err := cmd.Run(); err == nil {
		return "main", nil
	}

	// Try master
	cmd = exec.Command("git", "rev-parse", "--verify", "master")
	cmd.Dir = repoPath
	if err := cmd.Run(); err == nil {
		return "master", nil
	}

	// Try origin/main
	cmd = exec.Command("git", "rev-parse", "--verify", "origin/main")
	cmd.Dir = repoPath
	if err := cmd.Run(); err == nil {
		return "origin/main", nil
	}

	// Try origin/master
	cmd = exec.Command("git", "rev-parse", "--verify", "origin/master")
	cmd.Dir = repoPath
	if err := cmd.Run(); err == nil {
		return "origin/master", nil
	}

	return "", errors.New("could not detect default branch (main/master)")
}

// getMergeBase finds the merge-base between two refs.
func getMergeBase(repoPath, ref1, ref2 string) (string, error) {
	cmd := exec.Command("git", "merge-base", ref1, ref2)
	cmd.Dir = repoPath
	out, err := cmd.Output()
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

// getDiffStats returns lines added, deleted, and files modified.
func getDiffStats(repoPath, mergeBase string) (added, deleted, files int, err error) {
	cmd := exec.Command("git", "diff", "--numstat", mergeBase+"..HEAD")
	cmd.Dir = repoPath
	out, err := cmd.Output()
	if err != nil {
		return 0, 0, 0, err
	}

	scanner := bufio.NewScanner(bytes.NewReader(out))
	for scanner.Scan() {
		line := scanner.Text()
		parts := strings.Fields(line)
		if len(parts) >= 2 {
			if a, err := strconv.Atoi(parts[0]); err == nil {
				added += a
			}
			if d, err := strconv.Atoi(parts[1]); err == nil {
				deleted += d
			}
			files++
		}
	}

	return added, deleted, files, scanner.Err()
}

// getCommitCount returns the number of commits between merge-base and HEAD.
func getCommitCount(repoPath, mergeBase string) (int, error) {
	cmd := exec.Command("git", "rev-list", "--count", mergeBase+"..HEAD")
	cmd.Dir = repoPath
	out, err := cmd.Output()
	if err != nil {
		return 0, err
	}
	return strconv.Atoi(strings.TrimSpace(string(out)))
}

// getLinesPerFile returns lines changed per file for entropy calculation.
func getLinesPerFile(repoPath, mergeBase string) (map[string]int, error) {
	cmd := exec.Command("git", "diff", "--numstat", mergeBase+"..HEAD")
	cmd.Dir = repoPath
	out, err := cmd.Output()
	if err != nil {
		return nil, err
	}

	result := make(map[string]int)
	scanner := bufio.NewScanner(bytes.NewReader(out))
	for scanner.Scan() {
		line := scanner.Text()
		parts := strings.Fields(line)
		if len(parts) >= 3 {
			added, _ := strconv.Atoi(parts[0])
			deleted, _ := strconv.Atoi(parts[1])
			file := parts[2]
			result[file] = added + deleted
		}
	}

	return result, scanner.Err()
}

// ResolveRef resolves a reference to a commit hash.
func ResolveRef(repoPath, ref string) (plumbing.Hash, error) {
	cmd := exec.Command("git", "rev-parse", ref)
	cmd.Dir = repoPath
	out, err := cmd.Output()
	if err != nil {
		return plumbing.ZeroHash, err
	}
	return plumbing.NewHash(strings.TrimSpace(string(out))), nil
}
