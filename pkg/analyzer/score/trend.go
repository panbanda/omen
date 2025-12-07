package score

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"sync"
	"syscall"
	"time"

	"github.com/go-git/go-git/v5/plumbing"
	"github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/internal/vcs"
)

// TrendResult contains historical score data and computed statistics.
type TrendResult struct {
	// Raw data points (oldest first)
	Points []TrendPoint `json:"points"`

	// Overall score regression statistics
	Slope       float64 `json:"slope"`       // Score change per period
	Intercept   float64 `json:"intercept"`   // Y-intercept
	RSquared    float64 `json:"r_squared"`   // Goodness of fit (0-1)
	Correlation float64 `json:"correlation"` // Pearson correlation (-1 to 1)

	// Per-component trend statistics
	ComponentTrends ComponentTrends `json:"component_trends"`

	// Summary
	StartScore  int `json:"start_score"`
	EndScore    int `json:"end_score"`
	TotalChange int `json:"total_change"`

	// Metadata
	Period     string    `json:"period"`
	Since      string    `json:"since"`
	Snapped    bool      `json:"snapped"`
	AnalyzedAt time.Time `json:"analyzed_at"`
}

// ComponentTrends holds trend statistics for each score component.
type ComponentTrends struct {
	Complexity  TrendStats `json:"complexity"`
	Duplication TrendStats `json:"duplication"`
	Defect      TrendStats `json:"defect"`
	SATD        TrendStats `json:"satd"`
	TDG         TrendStats `json:"tdg"`
	Coupling    TrendStats `json:"coupling"`
	Smells      TrendStats `json:"smells"`
	Cohesion    TrendStats `json:"cohesion"`
}

// TrendPoint represents a score snapshot at a point in time.
type TrendPoint struct {
	Date       time.Time       `json:"date"`
	CommitSHA  string          `json:"commit_sha"`
	Score      int             `json:"score"`
	Components ComponentScores `json:"components"`
}

// TrendAnalyzer analyzes score trends over time.
type TrendAnalyzer struct {
	period      string
	since       time.Duration
	snap        bool
	weights     Weights
	thresholds  Thresholds
	churnDays   int
	maxFileSize int64
}

// TrendOption configures the TrendAnalyzer.
type TrendOption func(*TrendAnalyzer)

// WithTrendPeriod sets the sampling period (weekly or monthly).
func WithTrendPeriod(period string) TrendOption {
	return func(t *TrendAnalyzer) {
		t.period = period
	}
}

// WithTrendSince sets how far back to analyze.
func WithTrendSince(since time.Duration) TrendOption {
	return func(t *TrendAnalyzer) {
		t.since = since
	}
}

// WithTrendSnap enables snapping to period boundaries.
func WithTrendSnap(snap bool) TrendOption {
	return func(t *TrendAnalyzer) {
		t.snap = snap
	}
}

// WithTrendWeights sets custom weights for scoring.
func WithTrendWeights(w Weights) TrendOption {
	return func(t *TrendAnalyzer) {
		t.weights = w
	}
}

// WithTrendThresholds sets score thresholds.
func WithTrendThresholds(th Thresholds) TrendOption {
	return func(t *TrendAnalyzer) {
		t.thresholds = th
	}
}

// WithTrendChurnDays sets the git history period for churn analysis.
func WithTrendChurnDays(days int) TrendOption {
	return func(t *TrendAnalyzer) {
		t.churnDays = days
	}
}

// WithTrendMaxFileSize sets the maximum file size to analyze.
func WithTrendMaxFileSize(size int64) TrendOption {
	return func(t *TrendAnalyzer) {
		t.maxFileSize = size
	}
}

// NewTrendAnalyzer creates a new trend analyzer.
func NewTrendAnalyzer(opts ...TrendOption) *TrendAnalyzer {
	t := &TrendAnalyzer{
		period:    "monthly",
		since:     365 * 24 * time.Hour, // 1 year
		snap:      false,
		weights:   DefaultWeights(),
		churnDays: 30,
	}
	for _, opt := range opts {
		opt(t)
	}
	return t
}

// TrendProgressFunc reports progress during trend analysis.
type TrendProgressFunc func(current, total int, commitSHA string)

// AnalyzeTrend performs historical score analysis.
// Returns error if the working directory has uncommitted changes.
func (t *TrendAnalyzer) AnalyzeTrend(ctx context.Context, repoPath string) (*TrendResult, error) {
	return t.AnalyzeTrendWithProgress(ctx, repoPath, nil)
}

// AnalyzeTrendWithProgress performs historical score analysis with progress reporting.
func (t *TrendAnalyzer) AnalyzeTrendWithProgress(ctx context.Context, repoPath string, onProgress TrendProgressFunc) (*TrendResult, error) {
	// Check for detached HEAD state
	detached, err := vcs.IsDetachedHead(repoPath)
	if err != nil {
		return nil, fmt.Errorf("failed to check HEAD state: %w", err)
	}
	if detached {
		return nil, vcs.ErrDetachedHead
	}

	// Check for dirty working directory
	dirty, err := vcs.IsDirty(repoPath)
	if err != nil {
		return nil, fmt.Errorf("failed to check git status: %w", err)
	}
	if dirty {
		return nil, vcs.ErrDirtyWorkingDir
	}

	// Get current ref to restore later
	originalRef, err := vcs.GetCurrentRef(repoPath)
	if err != nil {
		return nil, fmt.Errorf("failed to get current ref: %w", err)
	}

	// Set up signal handler to restore on interrupt
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)
	defer signal.Stop(sigChan)

	var restoreOnce sync.Once
	restore := func() {
		restoreOnce.Do(func() {
			_ = vcs.CheckoutCommit(repoPath, originalRef)
		})
	}
	defer restore()

	// Handle interrupt via signal or context cancellation
	done := make(chan struct{})
	defer close(done)

	go func() {
		select {
		case <-sigChan:
			fmt.Fprintln(os.Stderr, "\nRestoring to original point in time...")
			restore()
			os.Exit(1)
		case <-ctx.Done():
			// Context canceled - restore handled by defer
		case <-done:
			// Normal completion - exit goroutine
		}
	}()

	// Find commits at intervals
	commits, err := vcs.FindCommitsAtIntervals(repoPath, t.period, t.since, t.snap)
	if err != nil {
		return nil, fmt.Errorf("failed to find commits: %w", err)
	}

	if len(commits) == 0 {
		return &TrendResult{
			Period:     t.period,
			Since:      formatDuration(t.since),
			Snapped:    t.snap,
			AnalyzedAt: time.Now().UTC(),
		}, nil
	}

	// Analyze each commit
	scanSvc := scanner.New()
	analyzer := New(
		WithWeights(t.weights),
		WithThresholds(t.thresholds),
		WithChurnDays(t.churnDays),
		WithMaxFileSize(t.maxFileSize),
	)

	var points []TrendPoint
	for i, commit := range commits {
		// Check for context cancellation
		select {
		case <-ctx.Done():
			fmt.Fprintln(os.Stderr, "\nCanceled. Restoring to original point in time...")
			return nil, ctx.Err()
		default:
		}

		if onProgress != nil {
			onProgress(i+1, len(commits), commit.SHA[:7])
		}

		// Checkout the commit
		if err := vcs.CheckoutCommit(repoPath, commit.SHA); err != nil {
			return nil, fmt.Errorf("failed to checkout %s: %w", commit.SHA[:7], err)
		}

		// Scan files at this commit
		scanResult, err := scanSvc.ScanPaths([]string{repoPath})
		if err != nil {
			continue // Skip commits with scan errors
		}

		if len(scanResult.Files) == 0 {
			continue
		}

		// Run score analysis
		result, err := analyzer.AnalyzeProject(ctx, repoPath, scanResult.Files)
		if err != nil {
			continue // Skip commits with analysis errors
		}

		points = append(points, TrendPoint{
			Date:       commit.Date,
			CommitSHA:  commit.SHA[:7],
			Score:      result.Score,
			Components: result.Components,
		})
	}

	// Restore original ref
	restore()

	// Build result
	trendResult := &TrendResult{
		Points:     points,
		Period:     t.period,
		Since:      formatDuration(t.since),
		Snapped:    t.snap,
		AnalyzedAt: time.Now().UTC(),
	}

	if len(points) > 0 {
		trendResult.StartScore = points[0].Score
		trendResult.EndScore = points[len(points)-1].Score
		trendResult.TotalChange = trendResult.EndScore - trendResult.StartScore

		// Compute overall score statistics
		stats := ComputeTrendStats(points)
		trendResult.Slope = stats.Slope
		trendResult.Intercept = stats.Intercept
		trendResult.RSquared = stats.RSquared
		trendResult.Correlation = stats.Correlation

		// Compute per-component statistics
		trendResult.ComponentTrends = ComputeComponentTrends(points)
	}

	return trendResult, nil
}

// formatDuration formats a duration as a human-readable string (3m, 6m, 1y, 2y).
func formatDuration(d time.Duration) string {
	days := int(d.Hours() / 24)
	switch {
	case days >= 730:
		return fmt.Sprintf("%dy", days/365)
	case days >= 365:
		return "1y"
	case days >= 180:
		return "6m"
	case days >= 90:
		return "3m"
	default:
		return fmt.Sprintf("%dm", days/30)
	}
}

// ParseSince parses a duration string like "3m", "6m", "1y", "2y".
func ParseSince(s string) (time.Duration, error) {
	if len(s) < 2 {
		return 0, fmt.Errorf("invalid duration: %s", s)
	}

	unit := s[len(s)-1]
	value := s[:len(s)-1]

	var n int
	if _, err := fmt.Sscanf(value, "%d", &n); err != nil {
		return 0, fmt.Errorf("invalid duration: %s", s)
	}

	switch unit {
	case 'm':
		return time.Duration(n) * 30 * 24 * time.Hour, nil
	case 'y':
		return time.Duration(n) * 365 * 24 * time.Hour, nil
	case 'w':
		return time.Duration(n) * 7 * 24 * time.Hour, nil
	case 'd':
		return time.Duration(n) * 24 * time.Hour, nil
	default:
		return 0, fmt.Errorf("invalid duration unit: %c (use m, y, w, or d)", unit)
	}
}

// AnalyzeTrendFast performs historical score analysis using go-git tree traversal.
// This is faster than AnalyzeTrend because it doesn't require checking out commits.
func (t *TrendAnalyzer) AnalyzeTrendFast(repo vcs.Repository) (*TrendResult, error) {
	return t.AnalyzeTrendFastWithProgress(repo, nil)
}

// AnalyzeTrendFastWithProgress performs historical score analysis with progress reporting.
func (t *TrendAnalyzer) AnalyzeTrendFastWithProgress(repo vcs.Repository, onProgress TrendProgressFunc) (*TrendResult, error) {
	// Find commits at intervals
	commits, err := findCommitsAtIntervalsFromRepo(repo, t.period, t.since, t.snap)
	if err != nil {
		return nil, fmt.Errorf("failed to find commits: %w", err)
	}

	if len(commits) == 0 {
		return &TrendResult{
			Period:     t.period,
			Since:      formatDuration(t.since),
			Snapped:    t.snap,
			AnalyzedAt: time.Now().UTC(),
		}, nil
	}

	// Create score analyzer
	analyzer := New(
		WithWeights(t.weights),
		WithThresholds(t.thresholds),
		WithChurnDays(t.churnDays),
		WithMaxFileSize(t.maxFileSize),
	)

	var points []TrendPoint
	for i, commit := range commits {
		if onProgress != nil {
			shortSHA := commit.SHA
			if len(shortSHA) > 7 {
				shortSHA = shortSHA[:7]
			}
			onProgress(i+1, len(commits), shortSHA)
		}

		// Get commit tree
		commitObj, err := repo.CommitObject(commit.Hash)
		if err != nil {
			continue
		}

		tree, err := commitObj.Tree()
		if err != nil {
			continue
		}

		// Get all analyzable files from the tree
		entries, err := tree.Entries()
		if err != nil {
			continue
		}

		var files []string
		for _, e := range entries {
			if !e.IsDir && isAnalyzableFile(e.Path) {
				files = append(files, e.Path)
			}
		}

		if len(files) == 0 {
			continue
		}

		// Create source from tree
		src := &treeSource{tree: tree}

		// Run score analysis
		result, err := analyzer.AnalyzeProjectFromSource(files, src, commit.SHA)
		if err != nil {
			continue
		}

		shortSHA := commit.SHA
		if len(shortSHA) > 7 {
			shortSHA = shortSHA[:7]
		}

		points = append(points, TrendPoint{
			Date:       commit.Date,
			CommitSHA:  shortSHA,
			Score:      result.Score,
			Components: result.Components,
		})
	}

	// Build result
	trendResult := &TrendResult{
		Points:     points,
		Period:     t.period,
		Since:      formatDuration(t.since),
		Snapped:    t.snap,
		AnalyzedAt: time.Now().UTC(),
	}

	if len(points) > 0 {
		trendResult.StartScore = points[0].Score
		trendResult.EndScore = points[len(points)-1].Score
		trendResult.TotalChange = trendResult.EndScore - trendResult.StartScore

		// Compute overall score statistics
		stats := ComputeTrendStats(points)
		trendResult.Slope = stats.Slope
		trendResult.Intercept = stats.Intercept
		trendResult.RSquared = stats.RSquared
		trendResult.Correlation = stats.Correlation

		// Compute per-component statistics
		trendResult.ComponentTrends = ComputeComponentTrends(points)
	}

	return trendResult, nil
}

// treeSource reads files from a git tree with thread-safe access.
type treeSource struct {
	tree vcs.Tree
	mu   sync.Mutex
}

func (t *treeSource) Read(path string) ([]byte, error) {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.tree.File(path)
}

// commitInfo holds commit information for interval sampling.
type commitInfo struct {
	SHA  string
	Hash plumbing.Hash
	Date time.Time
}

// findCommitsAtIntervalsFromRepo finds commits at regular intervals from a repository.
func findCommitsAtIntervalsFromRepo(repo vcs.Repository, period string, since time.Duration, snap bool) ([]commitInfo, error) {
	sinceTime := time.Now().Add(-since)

	iter, err := repo.Log(&vcs.LogOptions{Since: &sinceTime})
	if err != nil {
		return nil, err
	}
	defer iter.Close()

	// Collect all commits
	var allCommits []commitInfo
	err = iter.ForEach(func(c vcs.Commit) error {
		allCommits = append(allCommits, commitInfo{
			SHA:  c.Hash().String(),
			Hash: c.Hash(),
			Date: c.Author().When,
		})
		return nil
	})
	if err != nil {
		return nil, err
	}

	if len(allCommits) == 0 {
		return nil, nil
	}

	// Reverse to oldest first
	for i, j := 0, len(allCommits)-1; i < j; i, j = i+1, j-1 {
		allCommits[i], allCommits[j] = allCommits[j], allCommits[i]
	}

	// Sample at intervals
	return sampleCommitsAtIntervals(allCommits, period, snap), nil
}

// sampleCommitsAtIntervals samples commits at period intervals.
func sampleCommitsAtIntervals(commits []commitInfo, period string, snap bool) []commitInfo {
	if len(commits) == 0 {
		return nil
	}

	var result []commitInfo
	var lastPeriod time.Time

	for _, c := range commits {
		periodStart := getPeriodStart(c.Date, period, snap)

		if lastPeriod.IsZero() || !periodStart.Equal(lastPeriod) {
			result = append(result, c)
			lastPeriod = periodStart
		}
	}

	return result
}

// getPeriodStart returns the start of the period containing the given time.
// Always uses UTC to ensure consistent period bucketing across time zones.
func getPeriodStart(t time.Time, period string, snap bool) time.Time {
	// Normalize to UTC for consistent period calculation
	t = t.UTC()

	if !snap {
		// Without snapping, just truncate to the period
		switch period {
		case "daily":
			return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC)
		case "weekly":
			weekday := int(t.Weekday())
			if weekday == 0 {
				weekday = 7 // Sunday
			}
			return time.Date(t.Year(), t.Month(), t.Day()-weekday+1, 0, 0, 0, 0, time.UTC)
		case "monthly":
			return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, time.UTC)
		default:
			return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, time.UTC)
		}
	}

	// With snapping, align to calendar boundaries
	switch period {
	case "daily":
		return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, time.UTC)
	case "weekly":
		weekday := int(t.Weekday())
		if weekday == 0 {
			weekday = 7
		}
		return time.Date(t.Year(), t.Month(), t.Day()-weekday+1, 0, 0, 0, 0, time.UTC)
	case "monthly":
		return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, time.UTC)
	default:
		return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, time.UTC)
	}
}

// isAnalyzableFile checks if a file should be included in analysis.
func isAnalyzableFile(path string) bool {
	// Skip hidden files and directories
	for _, part := range splitPath(path) {
		if len(part) > 0 && part[0] == '.' {
			return false
		}
	}

	// Skip common non-source directories
	for _, part := range splitPath(path) {
		switch part {
		case "vendor", "node_modules", "dist", "build", ".git", "__pycache__", "target":
			return false
		}
	}

	// Check for known source extensions
	ext := getFileExtension(path)
	switch ext {
	case ".go", ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".java", ".c", ".cpp", ".h", ".hpp",
		".cs", ".rb", ".php", ".sh", ".bash":
		return true
	default:
		return false
	}
}

func splitPath(path string) []string {
	var parts []string
	current := ""
	for _, c := range path {
		if c == '/' || c == '\\' {
			if current != "" {
				parts = append(parts, current)
				current = ""
			}
		} else {
			current += string(c)
		}
	}
	if current != "" {
		parts = append(parts, current)
	}
	return parts
}

func getFileExtension(path string) string {
	for i := len(path) - 1; i >= 0; i-- {
		if path[i] == '.' {
			return path[i:]
		}
		if path[i] == '/' || path[i] == '\\' {
			break
		}
	}
	return ""
}
