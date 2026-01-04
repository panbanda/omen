package main

import (
	"context"
	"fmt"
	"sort"
	"strings"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/spf13/cobra"
)

var tdgCmd = &cobra.Command{
	Use:   "tdg [path...]",
	Short: "Calculate Technical Debt Gradient scores (0-100, higher is better)",
	RunE:  runTDG,
}

func init() {
	tdgCmd.Flags().Int("hotspots", 10, "Number of hotspots to show")
	tdgCmd.Flags().Bool("penalties", false, "Show applied penalties")

	analyzeCmd.AddCommand(tdgCmd)
}

func runTDG(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	hotspots, _ := cmd.Flags().GetInt("hotspots")
	showPenalties, _ := cmd.Flags().GetBool("penalties")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Calculating TDG scores...", len(scanResult.Files))
	svc := analysis.New()
	project, err := svc.AnalyzeTDG(context.Background(), scanResult.Files)
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, fmtErr := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if fmtErr != nil {
		return fmtErr
	}
	defer formatter.Close()

	// For JSON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON {
		report := project.ToReport(hotspots)
		return formatter.Output(report)
	}

	// Sort by score (lowest first - worst quality)
	files := project.Files
	sort.Slice(files, func(i, j int) bool {
		return files[i].Total < files[j].Total
	})

	// Show top hotspots (worst scores)
	filesToShow := files
	if len(filesToShow) > hotspots {
		filesToShow = filesToShow[:hotspots]
	}

	var rows [][]string
	for _, ts := range filesToShow {
		scoreStr := fmt.Sprintf("%.1f", ts.Total)
		gradeStr := string(ts.Grade)

		switch {
		case ts.Total >= 90:
			scoreStr = color.GreenString(scoreStr)
			gradeStr = color.GreenString(gradeStr)
		case ts.Total >= 70:
			scoreStr = color.YellowString(scoreStr)
			gradeStr = color.YellowString(gradeStr)
		default:
			scoreStr = color.RedString(scoreStr)
			gradeStr = color.RedString(gradeStr)
		}

		row := []string{
			ts.FilePath,
			scoreStr,
			gradeStr,
			fmt.Sprintf("%.1f", ts.StructuralComplexity),
			fmt.Sprintf("%.1f", ts.SemanticComplexity),
			fmt.Sprintf("%.1f", ts.CouplingScore),
		}

		if showPenalties && len(ts.PenaltiesApplied) > 0 {
			var penaltyStrs []string
			for _, p := range ts.PenaltiesApplied {
				penaltyStrs = append(penaltyStrs, fmt.Sprintf("%.1f: %s", p.Amount, p.Issue))
			}
			row = append(row, strings.Join(penaltyStrs, "; "))
		}

		rows = append(rows, row)
	}

	headers := []string{"File", "Score", "Grade", "Struct", "Semantic", "Coupling"}
	if showPenalties {
		headers = append(headers, "Penalties")
	}

	table := output.NewTable(
		fmt.Sprintf("TDG Analysis (Top %d Hotspots)", hotspots),
		headers,
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", project.TotalFiles),
			fmt.Sprintf("Average Score: %.1f", project.AverageScore),
			fmt.Sprintf("Average Grade: %s", project.AverageGrade),
		},
		project,
	)

	return formatter.Output(table)
}
