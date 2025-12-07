package main

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/urfave/cli/v2"
)

func trendCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.StringFlag{
			Name:    "since",
			Aliases: []string{"s"},
			Value:   "3m",
			Usage:   "How far back to analyze (e.g., 3m, 6m, 1y, 2y, 30d, 4w)",
		},
		&cli.StringFlag{
			Name:    "period",
			Aliases: []string{"p"},
			Value:   "weekly",
			Usage:   "Sampling period (daily, weekly, monthly)",
		},
		&cli.BoolFlag{
			Name:  "snap",
			Usage: "Snap to period boundaries (1st of month, Monday)",
		},
	)
	return &cli.Command{
		Name:      "trend",
		Aliases:   []string{"tr"},
		Usage:     "Analyze repository score over time",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runTrendCmd,
	}
}

func runTrendCmd(c *cli.Context) error {
	sinceStr := c.String("since")
	period := c.String("period")
	snap := c.Bool("snap")

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

	paths := getPaths(c)
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

	var tracker *progress.Tracker

	ctx := context.Background()
	result, err := trendAnalyzer.AnalyzeTrendWithProgress(ctx, repoPath, func(current, total int, commitSHA string) {
		if tracker == nil {
			tracker = progress.NewTracker(fmt.Sprintf("Analyzing %d points in time", total), total)
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

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	return formatter.Output(result)
}
