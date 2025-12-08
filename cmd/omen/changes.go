package main

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	"github.com/panbanda/omen/pkg/analyzer/changes"
	"github.com/spf13/cobra"
)

var changesCmd = &cobra.Command{
	Use:     "changes [path]",
	Aliases: []string{"jit"},
	Short:   "Analyze recent changes for defect risk (Kamei et al. 2013)",
	RunE:    runChanges,
}

func init() {
	changesCmd.Flags().Int("days", 30, "Number of days of history to analyze")
	changesCmd.Flags().Int("top", 20, "Show top N commits by risk")
	changesCmd.Flags().Bool("high-risk-only", false, "Show only high-risk commits")

	analyzeCmd.AddCommand(changesCmd)
}

func runChanges(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	days, _ := cmd.Flags().GetInt("days")
	topN, _ := cmd.Flags().GetInt("top")
	highRiskOnly, _ := cmd.Flags().GetBool("high-risk-only")

	if err := validateDays(days); err != nil {
		return err
	}

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing recent changes...")
	svc := analysis.New()
	result, err := svc.AnalyzeChanges(context.Background(), absPath, analysis.ChangesOptions{
		Days: days,
	})
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("changes analysis failed (is this a git repository?): %w", err)
	}

	if result.Summary.TotalCommits == 0 {
		color.Yellow("No commits found in the last %d days", days)
		return nil
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	commits := result.Commits
	if len(commits) > topN {
		commits = commits[:topN]
	}

	var rows [][]string
	for _, cr := range commits {
		if highRiskOnly && cr.RiskLevel != changes.RiskLevelHigh {
			continue
		}

		scoreStr := fmt.Sprintf("%.2f", cr.RiskScore)
		riskStr := string(cr.RiskLevel)
		switch cr.RiskLevel {
		case changes.RiskLevelHigh:
			scoreStr = color.RedString(scoreStr)
			riskStr = color.RedString(riskStr)
		case changes.RiskLevelMedium:
			scoreStr = color.YellowString(scoreStr)
			riskStr = color.YellowString(riskStr)
		case changes.RiskLevelLow:
			scoreStr = color.GreenString(scoreStr)
			riskStr = color.GreenString(riskStr)
		}

		rows = append(rows, []string{
			cr.CommitHash[:8],
			cr.Author,
			cr.Message,
			scoreStr,
			riskStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Change Risk Analysis (Last %d Days)", days),
		[]string{"Commit", "Author", "Message", "Risk Score", "Level"},
		rows,
		[]string{
			fmt.Sprintf("Total Commits: %d", result.Summary.TotalCommits),
			fmt.Sprintf("High Risk: %d", result.Summary.HighRiskCount),
			fmt.Sprintf("Medium Risk: %d", result.Summary.MediumRiskCount),
			fmt.Sprintf("Bug Fixes: %d", result.Summary.BugFixCount),
			fmt.Sprintf("Avg Score: %.2f", result.Summary.AvgRiskScore),
		},
		result,
	)

	return formatter.Output(table)
}
