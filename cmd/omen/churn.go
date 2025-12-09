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

var churnCmd = &cobra.Command{
	Use:   "churn [path...]",
	Short: "Analyze git commit history for file churn",
	RunE:  runChurn,
}

func init() {
	churnCmd.Flags().Int("days", 30, "Number of days of history to analyze")
	churnCmd.Flags().Int("top", 20, "Show top N files by churn")

	analyzeCmd.AddCommand(churnCmd)
}

func runChurn(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	days, _ := cmd.Flags().GetInt("days")
	topN, _ := cmd.Flags().GetInt("top")

	if err := validateDays(days); err != nil {
		return err
	}

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing git history...")
	svc := analysis.New()
	result, err := svc.AnalyzeChurn(context.Background(), absPath, analysis.ChurnOptions{
		Days:    days,
		Top:     topN,
		Spinner: spinner,
	})
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("churn analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	files := result.Files
	if len(files) > topN {
		files = files[:topN]
	}

	var rows [][]string
	for _, fm := range files {
		scoreStr := fmt.Sprintf("%.2f", fm.ChurnScore)
		if fm.ChurnScore >= 0.8 {
			scoreStr = color.RedString(scoreStr)
		} else if fm.ChurnScore >= 0.5 {
			scoreStr = color.YellowString(scoreStr)
		}

		rows = append(rows, []string{
			fm.Path,
			fmt.Sprintf("%d", fm.Commits),
			fmt.Sprintf("%d", len(fm.UniqueAuthors)),
			fmt.Sprintf("+%d/-%d", fm.LinesAdded, fm.LinesDeleted),
			scoreStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("File Churn (Last %d Days)", days),
		[]string{"File", "Commits", "Authors", "Lines Changed", "Churn Score"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFilesChanged),
			fmt.Sprintf("File Changes: %d", result.Summary.TotalFileChanges),
			fmt.Sprintf("Authors: %d", len(result.Summary.AuthorContributions)),
			"",
			fmt.Sprintf("Max: %.2f", result.Summary.MaxChurnScore),
		},
		result,
	)

	return formatter.Output(table)
}
