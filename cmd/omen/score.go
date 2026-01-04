package main

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/source"
	"github.com/spf13/cobra"
)

var scoreCmd = &cobra.Command{
	Use:   "score [path...]",
	Short: "Compute repository health score",
	RunE:  runScore,
}

var scoreTrendCmd = &cobra.Command{
	Use:   "trend [path]",
	Short: "Analyze score trends over time",
	RunE:  runScoreTrend,
}

func init() {
	scoreCmd.PersistentFlags().StringP("format", "f", "text", "Output format: text, json, markdown")
	scoreCmd.PersistentFlags().StringP("output", "o", "", "Write output to file")

	scoreCmd.Flags().Int("min-score", 0, "Minimum composite score (0-100)")
	scoreCmd.Flags().Int("min-complexity", 0, "Minimum complexity score (0-100)")
	scoreCmd.Flags().Int("min-duplication", 0, "Minimum duplication score (0-100)")
	scoreCmd.Flags().Int("min-defect", 0, "Minimum defect risk score (0-100)")
	scoreCmd.Flags().Int("min-debt", 0, "Minimum technical debt score (0-100)")
	scoreCmd.Flags().Int("min-coupling", 0, "Minimum coupling score (0-100)")
	scoreCmd.Flags().Int("min-smells", 0, "Minimum smells score (0-100)")
	scoreCmd.Flags().Int("min-cohesion", 0, "Minimum cohesion score (0-100)")
	scoreCmd.Flags().Bool("enable-cohesion", false, "Include cohesion in composite score (redistributes weights)")
	scoreCmd.Flags().Int("days", 30, "Days of git history for churn analysis")

	scoreTrendCmd.Flags().String("period", "monthly", "Sampling period: weekly or monthly")
	scoreTrendCmd.Flags().String("since", "1y", "How far back to analyze (e.g., 3m, 6m, 1y, 2y)")
	scoreTrendCmd.Flags().Bool("snap", false, "Snap to period boundaries (1st of month, Monday of week)")

	scoreCmd.AddCommand(scoreTrendCmd)
	rootCmd.AddCommand(scoreCmd)
}

func runScore(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)

	cfg, err := config.LoadOrDefault()
	if err != nil {
		return err
	}

	// CLI flag overrides config
	if enableCohesion, _ := cmd.Flags().GetBool("enable-cohesion"); enableCohesion {
		cfg.Score.EnableCohesion = true
	}

	// Build thresholds from flags (override config)
	thresholds := score.Thresholds{
		Score:       intFlagOrConfig(cmd, "min-score", cfg.Score.Thresholds.Score),
		Complexity:  intFlagOrConfig(cmd, "min-complexity", cfg.Score.Thresholds.Complexity),
		Duplication: intFlagOrConfig(cmd, "min-duplication", cfg.Score.Thresholds.Duplication),
		SATD:        intFlagOrConfig(cmd, "min-satd", cfg.Score.Thresholds.SATD),
		TDG:         intFlagOrConfig(cmd, "min-tdg", cfg.Score.Thresholds.TDG),
		Coupling:    intFlagOrConfig(cmd, "min-coupling", cfg.Score.Thresholds.Coupling),
		Smells:      intFlagOrConfig(cmd, "min-smells", cfg.Score.Thresholds.Smells),
		Cohesion:    intFlagOrConfig(cmd, "min-cohesion", cfg.Score.Thresholds.Cohesion),
	}

	// Build weights from effective config (handles enable_cohesion)
	effectiveWeights := cfg.Score.EffectiveWeights()
	weights := score.Weights{
		Complexity:  effectiveWeights.Complexity,
		Duplication: effectiveWeights.Duplication,
		SATD:        effectiveWeights.SATD,
		TDG:         effectiveWeights.TDG,
		Coupling:    effectiveWeights.Coupling,
		Smells:      effectiveWeights.Smells,
		Cohesion:    effectiveWeights.Cohesion,
	}

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	days, _ := cmd.Flags().GetInt("days")
	analyzer := score.New(
		score.WithWeights(weights),
		score.WithThresholds(thresholds),
		score.WithChurnDays(days),
		score.WithMaxFileSize(cfg.Analysis.MaxFileSize),
	)

	result, err := analyzer.Analyze(context.Background(), scanResult.Files, source.NewFilesystem(), "")
	if err != nil {
		return fmt.Errorf("score analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Output based on format
	format := getFormat(cmd)
	if format == "json" {
		if err := formatter.Output(result); err != nil {
			return err
		}
	} else if format == "markdown" {
		if err := writeScoreMarkdown(result, formatter); err != nil {
			return err
		}
	} else {
		printScoreResult(result)
	}

	// Exit with error if thresholds not met
	if !result.Passed {
		os.Exit(1)
	}
	return nil
}

// intFlagOrConfig returns the CLI flag value if explicitly set, otherwise the config value.
func intFlagOrConfig(cmd *cobra.Command, flagName string, cfgValue int) int {
	if cmd.Flags().Changed(flagName) {
		val, _ := cmd.Flags().GetInt(flagName)
		return val
	}
	return cfgValue
}

func printScoreResult(r *score.Result) {
	// Header with score coloring
	scoreColor := color.New(color.FgGreen)
	if r.Score < 70 {
		scoreColor = color.New(color.FgYellow)
	}
	if r.Score < 50 {
		scoreColor = color.New(color.FgRed)
	}

	fmt.Println()
	scoreColor.Printf("Repository Score: %d/100\n", r.Score)
	fmt.Println()

	// Components
	printScoreComponent("Complexity", r.Components.Complexity)
	printScoreComponent("Duplication", r.Components.Duplication)
	printScoreComponent("SATD", r.Components.SATD)
	printScoreComponent("TDG", r.Components.TDG)
	printScoreComponent("Coupling", r.Components.Coupling)
	printScoreComponent("Smells", r.Components.Smells)
	if r.CohesionIncluded {
		printScoreComponent("Cohesion", r.Components.Cohesion)
	}
	fmt.Println()
	fmt.Printf("Files analyzed: %d\n", r.FilesAnalyzed)

	// Threshold failures
	if !r.Passed {
		fmt.Println()
		color.Red("Threshold violations:")
		for name, th := range r.Thresholds {
			if !th.Passed {
				actual := getScoreComponent(r, name)
				color.Red("  %s: %d < %d (minimum)", name, actual, th.Min)
			}
		}
		fmt.Println()
		fmt.Println("Run analyzers to diagnose:")
		for name, th := range r.Thresholds {
			if !th.Passed && name != "score" {
				fmt.Printf("  omen analyze %s\n", componentToAnalyzer(name))
			}
		}
	}
}

func componentToAnalyzer(name string) string {
	switch name {
	case "complexity":
		return "complexity"
	case "duplication":
		return "duplicates"
	case "satd":
		return "satd"
	case "tdg":
		return "tdg"
	case "coupling":
		return "graph"
	case "smells":
		return "smells"
	case "cohesion":
		return "cohesion"
	default:
		return name
	}
}

func printScoreComponent(name string, value int) {
	c := color.New(color.FgGreen)
	if value < 70 {
		c = color.New(color.FgYellow)
	}
	if value < 50 {
		c = color.New(color.FgRed)
	}
	c.Printf("  %-15s %3d/100\n", name+":", value)
}

func getScoreComponent(r *score.Result, name string) int {
	switch name {
	case "score":
		return r.Score
	case "complexity":
		return r.Components.Complexity
	case "duplication":
		return r.Components.Duplication
	case "satd":
		return r.Components.SATD
	case "tdg":
		return r.Components.TDG
	case "coupling":
		return r.Components.Coupling
	case "smells":
		return r.Components.Smells
	case "cohesion":
		return r.Components.Cohesion
	default:
		return 0
	}
}

func writeScoreMarkdown(r *score.Result, f *output.Formatter) error {
	w := f.Writer()
	fmt.Fprintf(w, "# Repository Score: %d/100\n\n", r.Score)
	fmt.Fprintln(w, "## Component Scores")
	fmt.Fprintln(w)
	fmt.Fprintln(w, "| Component | Score |")
	fmt.Fprintln(w, "|-----------|-------|")
	fmt.Fprintf(w, "| Complexity | %d/100 |\n", r.Components.Complexity)
	fmt.Fprintf(w, "| Duplication | %d/100 |\n", r.Components.Duplication)
	fmt.Fprintf(w, "| SATD | %d/100 |\n", r.Components.SATD)
	fmt.Fprintf(w, "| TDG | %d/100 |\n", r.Components.TDG)
	fmt.Fprintf(w, "| Coupling | %d/100 |\n", r.Components.Coupling)
	fmt.Fprintf(w, "| Smells | %d/100 |\n", r.Components.Smells)
	if r.CohesionIncluded {
		fmt.Fprintf(w, "| Cohesion | %d/100 |\n", r.Components.Cohesion)
	}
	fmt.Fprintf(w, "\nFiles analyzed: %d\n", r.FilesAnalyzed)
	if r.Commit != "" {
		fmt.Fprintf(w, "Commit: %s\n", r.Commit)
	}
	fmt.Fprintf(w, "Timestamp: %s\n", r.Timestamp.Format(time.RFC3339))

	if !r.Passed {
		fmt.Fprintln(w)
		fmt.Fprintln(w, "## Threshold Violations")
		fmt.Fprintln(w)
		for name, th := range r.Thresholds {
			if !th.Passed {
				actual := getScoreComponent(r, name)
				fmt.Fprintf(w, "- **%s**: %d < %d (minimum)\n", name, actual, th.Min)
			}
		}
	}

	return nil
}

func runScoreTrend(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	repoPath := "."
	if len(paths) > 0 {
		repoPath = paths[0]
	}

	cfg, err := config.LoadOrDefault()
	if err != nil {
		return err
	}

	// Parse since duration
	sinceStr, _ := cmd.Flags().GetString("since")
	since, err := score.ParseSince(sinceStr)
	if err != nil {
		return err
	}

	// Build weights from effective config
	effectiveWeights := cfg.Score.EffectiveWeights()
	weights := score.Weights{
		Complexity:  effectiveWeights.Complexity,
		Duplication: effectiveWeights.Duplication,
		SATD:        effectiveWeights.SATD,
		TDG:         effectiveWeights.TDG,
		Coupling:    effectiveWeights.Coupling,
		Smells:      effectiveWeights.Smells,
		Cohesion:    effectiveWeights.Cohesion,
	}

	period, _ := cmd.Flags().GetString("period")
	snap, _ := cmd.Flags().GetBool("snap")
	analyzer := score.NewTrendAnalyzer(
		score.WithTrendPeriod(period),
		score.WithTrendSince(since),
		score.WithTrendSnap(snap),
		score.WithTrendWeights(weights),
		score.WithTrendChurnDays(30),
		score.WithTrendMaxFileSize(cfg.Analysis.MaxFileSize),
	)

	var tracker *progress.Tracker

	result, err := analyzer.AnalyzeTrendWithProgress(context.Background(), repoPath, func(current, total int, commitSHA string, fileCount int) {
		if tracker == nil {
			tracker = progress.NewTracker(fmt.Sprintf("Analyzing %d points in time", total), total)
		}
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
		return err
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	format := getFormat(cmd)
	switch format {
	case "json":
		if err := formatter.Output(result); err != nil {
			return err
		}
	case "markdown":
		if err := writeScoreTrendMarkdown(result, formatter); err != nil {
			return err
		}
	default:
		printScoreTrendResult(result)
	}

	return nil
}

func printScoreTrendResult(r *score.TrendResult) {
	if len(r.Points) == 0 {
		color.Yellow("No data points found in the specified time range")
		return
	}

	// Header
	fmt.Println()
	snapped := ""
	if r.Snapped {
		snapped = ", snapped"
	}
	fmt.Printf("Score Trend (%s, %s%s)\n", r.Period, r.Since, snapped)
	fmt.Println()

	// Table header
	fmt.Printf("%-12s %-9s %5s  %10s %11s %6s %4s %8s %6s %8s\n",
		"Date", "Commit", "Score", "Complexity", "Duplication", "SATD", "TDG", "Coupling", "Smells", "Cohesion")
	fmt.Println(strings.Repeat("-", 99))

	// Data rows
	for _, p := range r.Points {
		scoreColor := color.New(color.FgGreen)
		if p.Score < 70 {
			scoreColor = color.New(color.FgYellow)
		}
		if p.Score < 50 {
			scoreColor = color.New(color.FgRed)
		}

		fmt.Printf("%-12s %-9s ",
			p.Date.Format("2006-01-02"),
			p.CommitSHA)
		scoreColor.Printf("%5d", p.Score)
		fmt.Printf("  %10d %11d %6d %4d %8d %6d %8d\n",
			p.Components.Complexity,
			p.Components.Duplication,
			p.Components.SATD,
			p.Components.TDG,
			p.Components.Coupling,
			p.Components.Smells,
			p.Components.Cohesion)
	}

	// Summary
	fmt.Println()
	if len(r.Points) >= 2 {
		period := singularPeriod(r.Period)

		// Overall trend
		direction := "Stable"
		if r.Slope > 0.5 {
			direction = color.GreenString("Improving")
		} else if r.Slope < -0.5 {
			direction = color.RedString("Declining")
		}

		changeStr := fmt.Sprintf("%+d", r.TotalChange)
		if r.TotalChange > 0 {
			changeStr = color.GreenString(changeStr)
		} else if r.TotalChange < 0 {
			changeStr = color.RedString(changeStr)
		}

		fmt.Printf("Overall: %s (%s points, slope=%+.2f/%s, R²=%.2f)\n",
			direction, changeStr, r.Slope, period, r.RSquared)
		fmt.Println()

		// Per-component trends
		fmt.Println("Component Trends:")
		printComponentTrend("  Complexity", r.ComponentTrends.Complexity, period)
		printComponentTrend("  Duplication", r.ComponentTrends.Duplication, period)
		printComponentTrend("  SATD", r.ComponentTrends.SATD, period)
		printComponentTrend("  TDG", r.ComponentTrends.TDG, period)
		printComponentTrend("  Coupling", r.ComponentTrends.Coupling, period)
		printComponentTrend("  Smells", r.ComponentTrends.Smells, period)
		printComponentTrend("  Cohesion", r.ComponentTrends.Cohesion, period)
	} else {
		fmt.Println("Trend: Insufficient data points for trend analysis")
	}
}

func printComponentTrend(name string, stats score.TrendStats, period string) {
	direction := "stable"
	dirColor := color.New(color.Reset)
	if stats.Slope > 0.5 {
		direction = "improving"
		dirColor = color.New(color.FgGreen)
	} else if stats.Slope < -0.5 {
		direction = "declining"
		dirColor = color.New(color.FgRed)
	}

	fmt.Printf("%-14s ", name+":")
	dirColor.Printf("%-10s", direction)
	fmt.Printf(" (slope=%+.2f/%s, R²=%.2f)\n", stats.Slope, period, stats.RSquared)
}

func singularPeriod(period string) string {
	switch period {
	case "weekly":
		return "week"
	case "monthly":
		return "month"
	default:
		return period
	}
}

func writeScoreTrendMarkdown(r *score.TrendResult, f *output.Formatter) error {
	w := f.Writer()

	snapped := ""
	if r.Snapped {
		snapped = ", snapped"
	}
	fmt.Fprintf(w, "# Score Trend (%s, %s%s)\n\n", r.Period, r.Since, snapped)

	if len(r.Points) == 0 {
		fmt.Fprintln(w, "No data points found in the specified time range.")
		return nil
	}

	// Summary
	if len(r.Points) >= 2 {
		direction := "Stable"
		if r.Slope > 0.5 {
			direction = "Improving"
		} else if r.Slope < -0.5 {
			direction = "Declining"
		}

		fmt.Fprintf(w, "**Trend:** %s (%+d points, R²=%.2f)\n\n", direction, r.TotalChange, r.RSquared)
		fmt.Fprintf(w, "**Slope:** %+.2f points/%s\n\n", r.Slope, singularPeriod(r.Period))
	}

	// Table
	fmt.Fprintln(w, "| Date | Commit | Score | Complexity | Duplication | Defect | Debt | Coupling | Smells | Cohesion |")
	fmt.Fprintln(w, "|------|--------|-------|------------|-------------|--------|------|----------|--------|----------|")

	for _, p := range r.Points {
		fmt.Fprintf(w, "| %s | %s | %d | %d | %d | %d | %d | %d | %d | %d |\n",
			p.Date.Format("2006-01-02"),
			p.CommitSHA,
			p.Score,
			p.Components.Complexity,
			p.Components.Duplication,
			p.Components.SATD,
			p.Components.TDG,
			p.Components.Coupling,
			p.Components.Smells,
			p.Components.Cohesion)
	}

	fmt.Fprintf(w, "\n*Analyzed at: %s*\n", r.AnalyzedAt.Format(time.RFC3339))

	return nil
}
