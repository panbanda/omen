package main

import (
	"context"
	"fmt"
	"sort"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/spf13/cobra"
)

var lintHotspotCmd = &cobra.Command{
	Use:     "lint-hotspot [path...]",
	Aliases: []string{"lh"},
	Short:   "Identify files with high lint violation density",
	RunE:    runLintHotspot,
}

func init() {
	lintHotspotCmd.Flags().Int("top", 10, "Show top N files")

	analyzeCmd.AddCommand(lintHotspotCmd)
}

func runLintHotspot(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

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

	// Use complexity as hotspot indicator
	tracker := progress.NewTracker("Analyzing hotspots...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeComplexity(context.Background(), scanResult.Files, analysis.ComplexityOptions{
		OnProgress: tracker.Tick,
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

	// Sort by total complexity (as proxy for lint density)
	sort.Slice(result.Files, func(i, j int) bool {
		return result.Files[i].TotalCyclomatic+result.Files[i].TotalCognitive >
			result.Files[j].TotalCyclomatic+result.Files[j].TotalCognitive
	})

	filesToShow := result.Files
	if len(filesToShow) > topN {
		filesToShow = filesToShow[:topN]
	}

	var rows [][]string
	for _, fc := range filesToShow {
		score := fc.TotalCyclomatic + fc.TotalCognitive
		scoreStr := fmt.Sprintf("%d", score)
		if score > 100 {
			scoreStr = color.RedString(scoreStr)
		} else if score > 50 {
			scoreStr = color.YellowString(scoreStr)
		}

		rows = append(rows, []string{
			fc.Path,
			fmt.Sprintf("%d", len(fc.Functions)),
			fmt.Sprintf("%d", fc.TotalCyclomatic),
			fmt.Sprintf("%d", fc.TotalCognitive),
			scoreStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Complexity Hotspots (Top %d)", topN),
		[]string{"File", "Functions", "Cyclomatic", "Cognitive", "Total Score"},
		rows,
		nil,
		result,
	)

	return formatter.Output(table)
}
