package main

import (
	"context"
	"fmt"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/deadcode"
	"github.com/spf13/cobra"
)

var deadcodeCmd = &cobra.Command{
	Use:     "deadcode [path...]",
	Aliases: []string{"dc"},
	Short:   "Detect unused functions, variables, and unreachable code",
	RunE:    runDeadCode,
}

func init() {
	deadcodeCmd.Flags().Float64("confidence", 0.8, "Minimum confidence threshold (0.0-1.0)")

	analyzeCmd.AddCommand(deadcodeCmd)
}

func runDeadCode(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	confidence, _ := cmd.Flags().GetFloat64("confidence")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting dead code...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeDeadCode(context.Background(), scanResult.Files, analysis.DeadCodeOptions{
		Confidence: confidence,
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

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		pmatResult := deadcode.NewReport()
		pmatResult.FromAnalysis(result)
		return formatter.Output(pmatResult)
	}

	// Functions table
	if len(result.DeadFunctions) > 0 {
		var rows [][]string
		for _, fn := range result.DeadFunctions {
			confStr := fmt.Sprintf("%.0f%%", fn.Confidence*100)
			if fn.Confidence >= 0.9 {
				confStr = color.RedString(confStr)
			} else if fn.Confidence >= 0.8 {
				confStr = color.YellowString(confStr)
			}

			rows = append(rows, []string{
				fmt.Sprintf("%s:%d", fn.File, fn.Line),
				fn.Name,
				fn.Visibility,
				confStr,
			})
		}

		table := output.NewTable(
			"Potentially Dead Functions",
			[]string{"Location", "Function", "Visibility", "Confidence"},
			rows,
			nil,
			nil,
		)
		if err := formatter.Output(table); err != nil {
			return err
		}
	}

	// Variables table
	if len(result.DeadVariables) > 0 {
		var rows [][]string
		for _, v := range result.DeadVariables {
			rows = append(rows, []string{
				fmt.Sprintf("%s:%d", v.File, v.Line),
				v.Name,
				fmt.Sprintf("%.0f%%", v.Confidence*100),
			})
		}

		table := output.NewTable(
			"Potentially Dead Variables",
			[]string{"Location", "Variable", "Confidence"},
			rows,
			nil,
			nil,
		)
		if err := formatter.Output(table); err != nil {
			return err
		}
	}

	// Summary
	fmt.Printf("\nSummary: %d dead functions, %d dead variables across %d files (%.1f%% dead code)\n",
		result.Summary.TotalDeadFunctions,
		result.Summary.TotalDeadVariables,
		result.Summary.TotalFilesAnalyzed,
		result.Summary.DeadCodePercentage)

	return nil
}
