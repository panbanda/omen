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

var ownershipCmd = &cobra.Command{
	Use:     "ownership [path...]",
	Aliases: []string{"own", "bus-factor"},
	Short:   "Analyze code ownership and bus factor risk",
	RunE:    runOwnership,
}

func init() {
	ownershipCmd.Flags().Int("top", 20, "Show top N files by ownership concentration")
	ownershipCmd.Flags().Bool("include-trivial", false, "Include trivial lines (imports, braces, blanks) in ownership calculation")

	analyzeCmd.AddCommand(ownershipCmd)
}

func runOwnership(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	topN, _ := cmd.Flags().GetInt("top")
	includeTrivial, _ := cmd.Flags().GetBool("include-trivial")

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

	tracker := progress.NewTracker("Analyzing ownership", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeOwnership(context.Background(), repoPath, scanResult.Files, analysis.OwnershipOptions{
		Top:            topN,
		IncludeTrivial: includeTrivial,
		OnProgress:     tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("ownership analysis failed (is this a git repository?): %w", err)
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
	for _, fo := range filesToShow {
		concStr := fmt.Sprintf("%.2f", fo.Concentration)
		if fo.Concentration >= 0.9 {
			concStr = color.RedString(concStr)
		} else if fo.Concentration >= 0.7 {
			concStr = color.YellowString(concStr)
		}

		siloStr := ""
		if fo.IsSilo {
			siloStr = color.RedString("SILO")
		}

		rows = append(rows, []string{
			fo.Path,
			fo.PrimaryOwner,
			fmt.Sprintf("%.0f%%", fo.OwnershipPercent),
			concStr,
			fmt.Sprintf("%d", len(fo.Contributors)),
			siloStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Code Ownership (Top %d by Concentration)", topN),
		[]string{"Path", "Primary Owner", "Ownership", "Concentration", "Contributors", "Silo"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("Bus Factor: %d", result.Summary.BusFactor),
			fmt.Sprintf("Silos: %d", result.Summary.SiloCount),
			fmt.Sprintf("Avg Contributors: %.1f", result.Summary.AvgContributors),
		},
		result,
	)

	return formatter.Output(table)
}
