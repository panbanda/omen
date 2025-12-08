package main

import (
	"context"
	"fmt"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/spf13/cobra"
)

var smellsCmd = &cobra.Command{
	Use:   "smells [path...]",
	Short: "Detect architectural smells (cycles, hubs, god components, unstable dependencies)",
	RunE:  runSmells,
}

func init() {
	smellsCmd.Flags().Int("hub-threshold", 20, "Fan-in + fan-out threshold for hub detection")
	smellsCmd.Flags().Int("god-fan-in", 10, "Minimum fan-in for god component detection")
	smellsCmd.Flags().Int("god-fan-out", 10, "Minimum fan-out for god component detection")
	smellsCmd.Flags().Float64("instability-diff", 0.4, "Max instability difference for unstable dependency detection")

	analyzeCmd.AddCommand(smellsCmd)
}

func runSmells(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	hubThreshold, _ := cmd.Flags().GetInt("hub-threshold")
	godFanIn, _ := cmd.Flags().GetInt("god-fan-in")
	godFanOut, _ := cmd.Flags().GetInt("god-fan-out")
	instabilityDiff, _ := cmd.Flags().GetFloat64("instability-diff")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting architectural smells...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeSmells(context.Background(), scanResult.Files, analysis.SmellOptions{
		HubThreshold:          hubThreshold,
		GodFanInThreshold:     godFanIn,
		GodFanOutThreshold:    godFanOut,
		InstabilityDifference: instabilityDiff,
		OnProgress:            tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("smell analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	if len(result.Smells) == 0 {
		color.Green("No architectural smells detected")
		return nil
	}

	var rows [][]string
	for _, smell := range result.Smells {
		severityStr := string(smell.Severity)
		switch smell.Severity {
		case smells.SeverityCritical:
			severityStr = color.RedString(severityStr)
		case smells.SeverityHigh:
			severityStr = color.YellowString(severityStr)
		}

		componentsStr := smell.Components[0]
		if len(smell.Components) > 1 {
			componentsStr = fmt.Sprintf("%s (+%d more)", smell.Components[0], len(smell.Components)-1)
		}

		rows = append(rows, []string{
			string(smell.Type),
			severityStr,
			componentsStr,
			smell.Description,
		})
	}

	table := output.NewTable(
		"Architectural Smells",
		[]string{"Type", "Severity", "Components", "Description"},
		rows,
		[]string{
			fmt.Sprintf("Total Smells: %d", result.Summary.TotalSmells),
			fmt.Sprintf("Critical: %d", result.Summary.CriticalCount),
			fmt.Sprintf("High: %d", result.Summary.HighCount),
			fmt.Sprintf("Cycles: %d", result.Summary.CyclicCount),
			fmt.Sprintf("Hubs: %d", result.Summary.HubCount),
			fmt.Sprintf("Gods: %d", result.Summary.GodCount),
		},
		result,
	)

	return formatter.Output(table)
}
