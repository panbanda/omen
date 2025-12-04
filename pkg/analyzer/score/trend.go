package score

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/internal/vcs"
)

// TrendResult contains historical score data and computed statistics.
type TrendResult struct {
	// Raw data points (oldest first)
	Points []TrendPoint `json:"points"`

	// Regression statistics
	Slope       float64 `json:"slope"`       // Score change per period
	Intercept   float64 `json:"intercept"`   // Y-intercept
	RSquared    float64 `json:"r_squared"`   // Goodness of fit (0-1)
	Correlation float64 `json:"correlation"` // Pearson correlation (-1 to 1)

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

	restored := false
	restore := func() {
		if !restored {
			_ = vcs.CheckoutCommit(repoPath, originalRef)
			restored = true
		}
	}
	defer restore()

	// Handle interrupt
	go func() {
		<-sigChan
		restore()
		os.Exit(1)
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

		// Compute statistics
		stats := ComputeTrendStats(points)
		trendResult.Slope = stats.Slope
		trendResult.Intercept = stats.Intercept
		trendResult.RSquared = stats.RSquared
		trendResult.Correlation = stats.Correlation
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
