package main

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	"github.com/spf13/cobra"
)

var temporalCouplingCmd = &cobra.Command{
	Use:     "temporal-coupling [path...]",
	Aliases: []string{"tc"},
	Short:   "Identify files that frequently change together",
	RunE:    runTemporalCoupling,
}

func init() {
	temporalCouplingCmd.Flags().Int("top", 20, "Show top N file pairs by coupling strength")
	temporalCouplingCmd.Flags().Int("days", 30, "Number of days of git history to analyze")
	temporalCouplingCmd.Flags().Int("min-cochanges", 3, "Minimum number of co-changes to consider files coupled")

	analyzeCmd.AddCommand(temporalCouplingCmd)
}

func runTemporalCoupling(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	topN, _ := cmd.Flags().GetInt("top")
	days, _ := cmd.Flags().GetInt("days")
	minCochanges, _ := cmd.Flags().GetInt("min-cochanges")

	if err := validateDays(days); err != nil {
		return err
	}

	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing temporal coupling...")
	svc := analysis.New()
	result, err := svc.AnalyzeTemporalCoupling(context.Background(), repoPath, analysis.TemporalCouplingOptions{
		Days:         days,
		MinCochanges: minCochanges,
		Top:          topN,
	})
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("temporal coupling analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	couplingsToShow := result.Couplings
	if len(couplingsToShow) > topN {
		couplingsToShow = couplingsToShow[:topN]
	}

	var rows [][]string
	for _, fc := range couplingsToShow {
		strengthStr := fmt.Sprintf("%.2f", fc.CouplingStrength)
		if fc.CouplingStrength >= 0.7 {
			strengthStr = color.RedString(strengthStr)
		} else if fc.CouplingStrength >= 0.4 {
			strengthStr = color.YellowString(strengthStr)
		}

		rows = append(rows, []string{
			fc.FileA,
			fc.FileB,
			fmt.Sprintf("%d", fc.CochangeCount),
			strengthStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Temporal Coupling (Top %d, Last %d Days, Min %d Co-changes)", topN, days, minCochanges),
		[]string{"File A", "File B", "Co-changes", "Strength"},
		rows,
		[]string{
			fmt.Sprintf("Total Couplings: %d", result.Summary.TotalCouplings),
			fmt.Sprintf("Strong (>0.5): %d", result.Summary.StrongCouplings),
			fmt.Sprintf("Files Analyzed: %d", result.Summary.TotalFilesAnalyzed),
			fmt.Sprintf("Max Strength: %.2f", result.Summary.MaxCouplingStrength),
		},
		result,
	)

	return formatter.Output(table)
}
