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

var cohesionCmd = &cobra.Command{
	Use:     "cohesion [path...]",
	Aliases: []string{"ck"},
	Short:   "Analyze CK (Chidamber-Kemerer) OO metrics",
	RunE:    runCohesion,
}

func init() {
	cohesionCmd.Flags().Int("top", 20, "Show top N classes by LCOM (least cohesive first)")
	cohesionCmd.Flags().Bool("include-tests", false, "Include test files in analysis")
	cohesionCmd.Flags().String("sort", "lcom", "Sort by: lcom, wmc, cbo, dit")

	analyzeCmd.AddCommand(cohesionCmd)
}

func runCohesion(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	topN, _ := cmd.Flags().GetInt("top")
	includeTests, _ := cmd.Flags().GetBool("include-tests")
	sortBy := getSort(cmd, "lcom")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Analyzing CK metrics", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeCohesion(context.Background(), scanResult.Files, analysis.CohesionOptions{
		IncludeTests: includeTests,
		Sort:         sortBy,
		Top:          topN,
		OnProgress:   tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("cohesion analysis failed: %w", err)
	}

	if len(result.Classes) == 0 {
		color.Yellow("No OO classes found (CK metrics only apply to Java, Python, TypeScript, etc.)")
		return nil
	}

	// Sort by requested metric
	switch sortBy {
	case "wmc":
		result.SortByWMC()
	case "cbo":
		result.SortByCBO()
	case "dit":
		result.SortByDIT()
	default:
		result.SortByLCOM()
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	classesToShow := result.Classes
	if len(classesToShow) > topN {
		classesToShow = classesToShow[:topN]
	}

	var rows [][]string
	for _, cls := range classesToShow {
		lcomStr := fmt.Sprintf("%d", cls.LCOM)
		if cls.LCOM > 1 {
			lcomStr = color.YellowString(lcomStr)
		}
		if cls.LCOM > 3 {
			lcomStr = color.RedString(fmt.Sprintf("%d", cls.LCOM))
		}

		wmcStr := fmt.Sprintf("%d", cls.WMC)
		if cls.WMC > 30 {
			wmcStr = color.RedString(wmcStr)
		} else if cls.WMC > 15 {
			wmcStr = color.YellowString(wmcStr)
		}

		ditStr := fmt.Sprintf("%d", cls.DIT)
		if cls.DIT >= 5 {
			ditStr = color.RedString(ditStr)
		} else if cls.DIT >= 4 {
			ditStr = color.YellowString(ditStr)
		}

		nocStr := fmt.Sprintf("%d", cls.NOC)
		if cls.NOC >= 6 {
			nocStr = color.RedString(nocStr)
		} else if cls.NOC >= 4 {
			nocStr = color.YellowString(nocStr)
		}

		rows = append(rows, []string{
			cls.ClassName,
			cls.Path,
			wmcStr,
			fmt.Sprintf("%d", cls.CBO),
			fmt.Sprintf("%d", cls.RFC),
			lcomStr,
			ditStr,
			nocStr,
			fmt.Sprintf("%d", cls.NOM),
		})
	}

	table := output.NewTable(
		fmt.Sprintf("CK Metrics (Top %d by %s)", topN, sortBy),
		[]string{"Class", "Path", "WMC", "CBO", "RFC", "LCOM", "DIT", "NOC", "Methods"},
		rows,
		[]string{
			fmt.Sprintf("Total Classes: %d", result.Summary.TotalClasses),
			fmt.Sprintf("Low Cohesion (LCOM>1): %d", result.Summary.LowCohesionCount),
			fmt.Sprintf("Avg WMC: %.1f", result.Summary.AvgWMC),
			fmt.Sprintf("Max WMC: %d", result.Summary.MaxWMC),
			fmt.Sprintf("Max DIT: %d", result.Summary.MaxDIT),
		},
		result,
	)

	return formatter.Output(table)
}
