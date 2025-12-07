package main

import (
	"fmt"
	"time"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/vcs"
	commitanalyzer "github.com/panbanda/omen/pkg/analyzer/commit"
	"github.com/urfave/cli/v2"
)

func trendCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:    "days",
			Aliases: []string{"d"},
			Value:   30,
			Usage:   "Number of days to analyze",
		},
	)
	return &cli.Command{
		Name:      "trend",
		Aliases:   []string{"tr"},
		Usage:     "Analyze repository metrics over time",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runTrendCmd,
	}
}

func runTrendCmd(c *cli.Context) error {
	days := c.Int("days")

	if err := validateDays(days); err != nil {
		return err
	}

	paths := getPaths(c)

	opener := vcs.NewGitOpener()
	repo, err := opener.PlainOpenWithDetect(paths[0])
	if err != nil {
		return fmt.Errorf("failed to open repository: %w", err)
	}

	a := commitanalyzer.New()
	defer a.Close()

	duration := time.Duration(days) * 24 * time.Hour
	trends, err := a.AnalyzeTrend(repo, duration)
	if err != nil {
		return fmt.Errorf("failed to analyze trends: %w", err)
	}

	if len(trends.Commits) == 0 {
		color.Yellow("No commits found in the specified time range")
		return nil
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	return formatter.Output(trends)
}
