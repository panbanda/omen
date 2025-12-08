package main

import (
	"context"
	"fmt"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/spf13/cobra"
)

var satdCmd = &cobra.Command{
	Use:     "satd [path...]",
	Aliases: []string{"debt"},
	Short:   "Detect self-admitted technical debt (TODO, FIXME, HACK)",
	RunE:    runSATD,
}

func init() {
	satdCmd.Flags().StringSlice("patterns", nil, "Additional patterns to detect")
	satdCmd.Flags().Bool("include-test", false, "Include test files in analysis")

	analyzeCmd.AddCommand(satdCmd)
}

func runSATD(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	patterns, _ := cmd.Flags().GetStringSlice("patterns")
	includeTest, _ := cmd.Flags().GetBool("include-test")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	// Convert patterns to PatternConfig
	var customPatterns []analysis.PatternConfig
	for _, p := range patterns {
		customPatterns = append(customPatterns, analysis.PatternConfig{
			Pattern:  p,
			Category: satd.CategoryDesign,
			Severity: satd.SeverityMedium,
		})
	}

	tracker := progress.NewTracker("Detecting technical debt...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeSATD(context.Background(), scanResult.Files, analysis.SATDOptions{
		IncludeTests:   includeTest,
		CustomPatterns: customPatterns,
		OnProgress:     tracker.Tick,
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

	var rows [][]string
	for _, item := range result.Items {
		sevColor := string(item.Severity)
		switch item.Severity {
		case satd.SeverityHigh:
			sevColor = color.RedString(string(item.Severity))
		case satd.SeverityMedium:
			sevColor = color.YellowString(string(item.Severity))
		case satd.SeverityLow:
			sevColor = color.GreenString(string(item.Severity))
		}

		rows = append(rows, []string{
			fmt.Sprintf("%s:%d", item.File, item.Line),
			item.Marker,
			sevColor,
			truncate(item.Description, 60),
		})
	}

	table := output.NewTable(
		"Self-Admitted Technical Debt",
		[]string{"Location", "Marker", "Severity", "Description"},
		rows,
		[]string{
			fmt.Sprintf("Total: %d", result.Summary.TotalItems),
			fmt.Sprintf("High: %d", result.Summary.BySeverity["high"]),
			fmt.Sprintf("Medium: %d", result.Summary.BySeverity["medium"]),
			fmt.Sprintf("Low: %d", result.Summary.BySeverity["low"]),
		},
		result,
	)

	return formatter.Output(table)
}
