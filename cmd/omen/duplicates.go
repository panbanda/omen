package main

import (
	"context"
	"fmt"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/spf13/cobra"
)

var duplicatesCmd = &cobra.Command{
	Use:     "duplicates [path...]",
	Aliases: []string{"dup", "clones"},
	Short:   "Detect code clones and duplicates",
	RunE:    runDuplicates,
}

func init() {
	duplicatesCmd.Flags().Int("min-lines", 6, "Minimum lines for clone detection")
	duplicatesCmd.Flags().Float64("threshold", 0.8, "Similarity threshold (0.0-1.0)")

	analyzeCmd.AddCommand(duplicatesCmd)
}

func runDuplicates(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	minLines, _ := cmd.Flags().GetInt("min-lines")
	threshold, _ := cmd.Flags().GetFloat64("threshold")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting duplicates...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeDuplicates(context.Background(), scanResult.Files, analysis.DuplicatesOptions{
		MinLines:            minLines,
		SimilarityThreshold: threshold,
		OnProgress:          tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// For JSON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON {
		report := result.ToReport()
		return formatter.Output(report)
	}

	if len(result.Clones) == 0 {
		return formatter.Output(result)
	}

	var rows [][]string
	for _, clone := range result.Clones {
		simStr := fmt.Sprintf("%.0f%%", clone.Similarity*100)
		rows = append(rows, []string{
			fmt.Sprintf("%s:%d-%d", clone.FileA, clone.StartLineA, clone.EndLineA),
			fmt.Sprintf("%s:%d-%d", clone.FileB, clone.StartLineB, clone.EndLineB),
			string(clone.Type),
			simStr,
			fmt.Sprintf("%d", clone.LinesA),
		})
	}

	table := output.NewTable(
		"Code Clones Detected",
		[]string{"Location A", "Location B", "Type", "Similarity", "Lines"},
		rows,
		[]string{
			fmt.Sprintf("Total Clones: %d", result.Summary.TotalClones),
			fmt.Sprintf("Type-1: %d", result.Summary.Type1Count),
			fmt.Sprintf("Type-2: %d", result.Summary.Type2Count),
			fmt.Sprintf("Type-3: %d", result.Summary.Type3Count),
			fmt.Sprintf("Avg Sim: %.0f%%", result.Summary.AvgSimilarity*100),
		},
		result,
	)

	return formatter.Output(table)
}
