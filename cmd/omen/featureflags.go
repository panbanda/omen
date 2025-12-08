package main

import (
	"context"
	"fmt"
	"strings"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/featureflags"
	"github.com/panbanda/omen/pkg/config"
	"github.com/spf13/cobra"
)

var featureFlagsCmd = &cobra.Command{
	Use:     "flags [path...]",
	Aliases: []string{"ff"},
	Short:   "Detect and analyze feature flags",
	RunE:    runFeatureFlags,
}

func init() {
	featureFlagsCmd.Flags().StringSlice("provider", nil, "Filter by provider (launchdarkly, split, unleash, posthog, flipper)")
	featureFlagsCmd.Flags().Bool("no-git", false, "Skip git history analysis for staleness")
	featureFlagsCmd.Flags().String("min-priority", "", "Filter by minimum priority (LOW, MEDIUM, HIGH, CRITICAL)")
	featureFlagsCmd.Flags().Int("top", 0, "Show only top N flags by priority (0 = all)")

	analyzeCmd.AddCommand(featureFlagsCmd)
}

func runFeatureFlags(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	providers, _ := cmd.Flags().GetStringSlice("provider")
	noGit, _ := cmd.Flags().GetBool("no-git")
	minPriority, _ := cmd.Flags().GetString("min-priority")
	topN, _ := cmd.Flags().GetInt("top")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting feature flags...", len(scanResult.Files))

	// Load config from flag or default locations
	var svc *analysis.Service
	if configPath, _ := cmd.Flags().GetString("config"); configPath != "" {
		cfg, err := config.Load(configPath)
		if err != nil {
			return fmt.Errorf("failed to load config: %w", err)
		}
		svc = analysis.New(analysis.WithConfig(cfg))
	} else {
		svc = analysis.New()
	}

	result, err := svc.AnalyzeFeatureFlags(context.Background(), scanResult.Files, analysis.FeatureFlagOptions{
		Providers:  providers,
		IncludeGit: !noGit,
		OnProgress: tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("feature flag analysis failed: %w", err)
	}

	if len(result.Flags) == 0 {
		color.Yellow("No feature flags detected")
		return nil
	}

	// Filter by minimum priority if specified
	flags := result.Flags
	if minPriority != "" {
		minPriority = strings.ToUpper(minPriority)
		priorityOrder := map[string]int{
			"LOW": 0, "MEDIUM": 1, "HIGH": 2, "CRITICAL": 3,
		}
		minOrder, ok := priorityOrder[minPriority]
		if ok {
			filtered := make([]featureflags.FlagAnalysis, 0)
			for _, f := range flags {
				if priorityOrder[f.Priority.Level] >= minOrder {
					filtered = append(filtered, f)
				}
			}
			flags = filtered
		}
	}

	// Limit results if requested
	if topN > 0 && len(flags) > topN {
		flags = flags[:topN]
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	var rows [][]string
	for _, f := range flags {
		priorityStr := f.Priority.Level
		switch f.Priority.Level {
		case "CRITICAL":
			priorityStr = color.RedString(priorityStr)
		case "HIGH":
			priorityStr = color.YellowString(priorityStr)
		case "MEDIUM":
			priorityStr = color.CyanString(priorityStr)
		}

		staleness := "-"
		if f.Staleness != nil {
			staleness = fmt.Sprintf("%dd", f.Staleness.DaysSinceIntro)
		}

		rows = append(rows, []string{
			f.FlagKey,
			f.Provider,
			fmt.Sprintf("%d", len(f.References)),
			fmt.Sprintf("%d", f.Complexity.FileSpread),
			fmt.Sprintf("%d", f.Complexity.MaxNestingDepth),
			staleness,
			priorityStr,
		})
	}

	table := output.NewTable(
		"Feature Flag Analysis",
		[]string{"Flag Key", "Provider", "Refs", "Files", "Nesting", "Age", "Priority"},
		rows,
		[]string{
			fmt.Sprintf("Total Flags: %d", result.Summary.TotalFlags),
			fmt.Sprintf("Total References: %d", result.Summary.TotalReferences),
			fmt.Sprintf("Critical: %d, High: %d, Medium: %d, Low: %d",
				result.Summary.ByPriority["CRITICAL"],
				result.Summary.ByPriority["HIGH"],
				result.Summary.ByPriority["MEDIUM"],
				result.Summary.ByPriority["LOW"]),
		},
		result,
	)

	return formatter.Output(table)
}
