package main

import (
	"context"
	"fmt"
	"path/filepath"
	"sync"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/spf13/cobra"
)

var trendCmd = &cobra.Command{
	Use:     "trend [path...]",
	Aliases: []string{"tr"},
	Short:   "Analyze repository score over time",
	RunE:    runTrend,
}

func init() {
	trendCmd.Flags().StringP("since", "s", "3m", "How far back to analyze (e.g., 3m, 6m, 1y, 2y, 30d, 4w)")
	trendCmd.Flags().StringP("period", "p", "weekly", "Sampling period (daily, weekly, monthly)")
	trendCmd.Flags().Bool("snap", false, "Snap to period boundaries (1st of month, Monday)")

	analyzeCmd.AddCommand(trendCmd)
}

func runTrend(cmd *cobra.Command, args []string) error {
	sinceStr, _ := cmd.Flags().GetString("since")
	period, _ := cmd.Flags().GetString("period")
	snap, _ := cmd.Flags().GetBool("snap")

	// Parse duration
	since, err := score.ParseSince(sinceStr)
	if err != nil {
		return fmt.Errorf("invalid --since value: %w", err)
	}

	// Validate period
	switch period {
	case "daily", "weekly", "monthly":
		// OK
	default:
		return fmt.Errorf("invalid --period: %s (use daily, weekly, or monthly)", period)
	}

	paths := getPaths(args)
	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("failed to resolve path: %w", err)
	}

	// Create trend analyzer
	trendAnalyzer := score.NewTrendAnalyzer(
		score.WithTrendPeriod(period),
		score.WithTrendSince(since),
		score.WithTrendSnap(snap),
	)

	ctx := context.Background()
	var tracker *progress.Tracker
	var trackerOnce sync.Once

	result, err := trendAnalyzer.AnalyzeTrendWithProgress(ctx, repoPath, func(current, total int, commitSHA string, fileCount int) {
		trackerOnce.Do(func() {
			tracker = progress.NewTracker(fmt.Sprintf("Analyzing %d points in time", total), total)
		})
		if fileCount > 0 {
			tracker.SetDescription(fmt.Sprintf("%s (%d files)", commitSHA, fileCount))
		} else {
			tracker.SetDescription(commitSHA)
		}
		tracker.Tick()
	})
	if tracker != nil {
		if err != nil {
			tracker.FinishError(err)
		} else {
			tracker.FinishSuccess()
		}
	}
	if err != nil {
		return fmt.Errorf("failed to analyze trend: %w", err)
	}

	if len(result.Points) == 0 {
		color.Yellow("No commits found in the specified time range")
		return nil
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	return formatter.Output(result)
}
