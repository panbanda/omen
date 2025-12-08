package main

import (
	"context"
	"fmt"
	"path/filepath"
	"sort"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/defect"
	"github.com/spf13/cobra"
)

var defectCmd = &cobra.Command{
	Use:     "defect [path...]",
	Aliases: []string{"predict"},
	Short:   "Predict defect probability using PMAT weights",
	RunE:    runDefect,
}

func init() {
	defectCmd.Flags().Bool("high-risk-only", false, "Show only high-risk files")

	analyzeCmd.AddCommand(defectCmd)
}

func runDefect(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	highRiskOnly, _ := cmd.Flags().GetBool("high-risk-only")

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

	svc := analysis.New()
	result, err := svc.AnalyzeDefects(context.Background(), repoPath, scanResult.Files, analysis.DefectOptions{
		HighRiskOnly: highRiskOnly,
	})
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Sort by probability (highest first)
	sort.Slice(result.Files, func(i, j int) bool {
		return result.Files[i].Probability > result.Files[j].Probability
	})

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		report := result.ToReport()
		return formatter.Output(report)
	}

	var rows [][]string
	for _, ds := range result.Files {
		if highRiskOnly && ds.RiskLevel != defect.RiskHigh {
			continue
		}

		probStr := fmt.Sprintf("%.0f%%", ds.Probability*100)
		riskStr := string(ds.RiskLevel)
		switch ds.RiskLevel {
		case defect.RiskHigh:
			probStr = color.RedString(probStr)
			riskStr = color.RedString(riskStr)
		case defect.RiskMedium:
			probStr = color.YellowString(probStr)
			riskStr = color.YellowString(riskStr)
		case defect.RiskLow:
			probStr = color.GreenString(probStr)
			riskStr = color.GreenString(riskStr)
		}

		rows = append(rows, []string{
			ds.FilePath,
			probStr,
			riskStr,
		})
	}

	table := output.NewTable(
		"Defect Probability Prediction",
		[]string{"File", "Probability", "Risk Level"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("High Risk: %d", result.Summary.HighRiskCount),
			fmt.Sprintf("Medium Risk: %d", result.Summary.MediumRiskCount),
			fmt.Sprintf("Avg Prob: %.0f%%", result.Summary.AvgProbability*100),
		},
		result,
	)

	return formatter.Output(table)
}
