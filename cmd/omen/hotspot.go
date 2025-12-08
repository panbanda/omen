package main

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/spf13/cobra"
)

var hotspotCmd = &cobra.Command{
	Use:     "hotspot [path...]",
	Aliases: []string{"hs"},
	Short:   "Identify code hotspots (high churn + high complexity)",
	RunE:    runHotspot,
}

func init() {
	hotspotCmd.Flags().Int("top", 20, "Show top N files by hotspot score")
	hotspotCmd.Flags().Int("days", 30, "Number of days of git history to analyze")

	analyzeCmd.AddCommand(hotspotCmd)
}

func runHotspot(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	topN, _ := cmd.Flags().GetInt("top")
	days, _ := cmd.Flags().GetInt("days")

	if err := validateDays(days); err != nil {
		return err
	}

	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
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

	tracker := progress.NewTracker("Analyzing hotspots...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeHotspots(context.Background(), repoPath, scanResult.Files, analysis.HotspotOptions{
		Days:       days,
		Top:        topN,
		OnProgress: tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("hotspot analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	filesToShow := result.Files
	if len(filesToShow) > topN {
		filesToShow = filesToShow[:topN]
	}

	var rows [][]string
	for _, fh := range filesToShow {
		hotspotStr := fmt.Sprintf("%.2f", fh.HotspotScore)
		if fh.HotspotScore >= 0.7 {
			hotspotStr = color.RedString(hotspotStr)
		} else if fh.HotspotScore >= 0.4 {
			hotspotStr = color.YellowString(hotspotStr)
		}

		rows = append(rows, []string{
			fh.Path,
			hotspotStr,
			fmt.Sprintf("%.2f", fh.ChurnScore),
			fmt.Sprintf("%.2f", fh.ComplexityScore),
			fmt.Sprintf("%d", fh.Commits),
			fmt.Sprintf("%.1f", fh.AvgCognitive),
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Code Hotspots (Top %d, Last %d Days)", topN, days),
		[]string{"File", "Hotspot", "Churn", "Complexity", "Commits", "Avg Cognitive"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("Hotspots (>0.5): %d", result.Summary.HotspotCount),
			fmt.Sprintf("Max Score: %.2f", result.Summary.MaxHotspotScore),
			fmt.Sprintf("Avg Score: %.2f", result.Summary.AvgHotspotScore),
		},
		result,
	)

	return formatter.Output(table)
}
