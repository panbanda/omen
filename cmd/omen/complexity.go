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

var complexityCmd = &cobra.Command{
	Use:     "complexity [path...]",
	Aliases: []string{"cx"},
	Short:   "Analyze cyclomatic and cognitive complexity",
	RunE:    runComplexity,
}

func init() {
	complexityCmd.Flags().Int("cyclomatic-threshold", 10, "Cyclomatic complexity warning threshold")
	complexityCmd.Flags().Int("cognitive-threshold", 15, "Cognitive complexity warning threshold")
	complexityCmd.Flags().Bool("functions-only", false, "Show only function-level metrics")

	analyzeCmd.AddCommand(complexityCmd)
}

func runComplexity(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	cycThreshold, _ := cmd.Flags().GetInt("cyclomatic-threshold")
	cogThreshold, _ := cmd.Flags().GetInt("cognitive-threshold")
	functionsOnly, _ := cmd.Flags().GetBool("functions-only")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Analyzing complexity...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeComplexity(context.Background(), scanResult.Files, analysis.ComplexityOptions{
		CyclomaticThreshold: cycThreshold,
		CognitiveThreshold:  cogThreshold,
		FunctionsOnly:       functionsOnly,
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

	var rows [][]string
	var warnings []string

	for _, fc := range result.Files {
		if functionsOnly {
			for _, fn := range fc.Functions {
				cycColor := fmt.Sprintf("%d", fn.Metrics.Cyclomatic)
				cogColor := fmt.Sprintf("%d", fn.Metrics.Cognitive)

				if fn.Metrics.Cyclomatic > uint32(cycThreshold) {
					cycColor = color.RedString("%d", fn.Metrics.Cyclomatic)
					warnings = append(warnings, fmt.Sprintf("%s:%d %s - cyclomatic complexity %d exceeds threshold %d",
						fc.Path, fn.StartLine, fn.Name, fn.Metrics.Cyclomatic, cycThreshold))
				}
				if fn.Metrics.Cognitive > uint32(cogThreshold) {
					cogColor = color.RedString("%d", fn.Metrics.Cognitive)
					warnings = append(warnings, fmt.Sprintf("%s:%d %s - cognitive complexity %d exceeds threshold %d",
						fc.Path, fn.StartLine, fn.Name, fn.Metrics.Cognitive, cogThreshold))
				}

				rows = append(rows, []string{
					fc.Path,
					fn.Name,
					fmt.Sprintf("%d", fn.StartLine),
					cycColor,
					cogColor,
					fmt.Sprintf("%d", fn.Metrics.MaxNesting),
				})
			}
		} else {
			avgCyc := fmt.Sprintf("%.1f", fc.AvgCyclomatic)
			avgCog := fmt.Sprintf("%.1f", fc.AvgCognitive)

			if fc.AvgCyclomatic > float64(cycThreshold) {
				avgCyc = color.RedString("%.1f", fc.AvgCyclomatic)
			}
			if fc.AvgCognitive > float64(cogThreshold) {
				avgCog = color.RedString("%.1f", fc.AvgCognitive)
			}

			rows = append(rows, []string{
				fc.Path,
				fmt.Sprintf("%d", len(fc.Functions)),
				avgCyc,
				avgCog,
			})
		}
	}

	var headers []string
	if functionsOnly {
		headers = []string{"File", "Function", "Line", "Cyclomatic", "Cognitive", "Nesting"}
	} else {
		headers = []string{"File", "Functions", "Avg Cyclomatic", "Avg Cognitive"}
	}

	table := output.NewTable(
		"Complexity Analysis",
		headers,
		rows,
		[]string{
			fmt.Sprintf("Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("Functions: %d", result.Summary.TotalFunctions),
			fmt.Sprintf("Median Cyclomatic (P50): %d", result.Summary.P50Cyclomatic),
			fmt.Sprintf("Median Cognitive (P50): %d", result.Summary.P50Cognitive),
			fmt.Sprintf("90th Percentile Cyclomatic: %d", result.Summary.P90Cyclomatic),
			fmt.Sprintf("90th Percentile Cognitive: %d", result.Summary.P90Cognitive),
			fmt.Sprintf("Max Cyclomatic: %d", result.Summary.MaxCyclomatic),
			fmt.Sprintf("Max Cognitive: %d", result.Summary.MaxCognitive),
		},
		result,
	)

	if err := formatter.Output(table); err != nil {
		return err
	}

	if len(warnings) > 0 && formatter.Format() == output.FormatText {
		fmt.Println()
		color.Yellow("Warnings (%d):", len(warnings))
		for _, w := range warnings {
			fmt.Printf("  - %s\n", w)
		}
	}

	return nil
}
