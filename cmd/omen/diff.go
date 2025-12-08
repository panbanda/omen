package main

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/pkg/analyzer/changes"
	"github.com/spf13/cobra"
)

var diffCmd = &cobra.Command{
	Use:     "diff [path]",
	Aliases: []string{"pr"},
	Short:   "Analyze current branch diff for defect risk (PR risk assessment)",
	RunE:    runDiff,
}

func init() {
	diffCmd.Flags().StringP("target", "t", "", "Target branch to compare against (default: auto-detect main/master)")
	diffCmd.Flags().Int("days", 90, "Days of history for normalization")

	analyzeCmd.AddCommand(diffCmd)
}

func runDiff(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	target, _ := cmd.Flags().GetString("target")
	days, _ := cmd.Flags().GetInt("days")

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing branch diff...")
	analyzer := changes.New(
		changes.WithDays(days),
	)
	defer analyzer.Close()

	result, err := analyzer.AnalyzeDiff(absPath, target)
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("diff analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	format := getFormat(cmd)
	if format == "json" || format == "toon" {
		return formatter.Output(result)
	}

	// Text/markdown output
	printDiffResult(result)
	return nil
}

func printDiffResult(r *changes.DiffResult) {
	var levelColor *color.Color
	switch r.Level {
	case "high":
		levelColor = color.New(color.FgRed)
	case "medium":
		levelColor = color.New(color.FgYellow)
	default:
		levelColor = color.New(color.FgGreen)
	}

	fmt.Println()
	fmt.Printf("Branch Diff Risk Analysis\n")
	fmt.Printf("==========================\n\n")
	fmt.Printf("Source:   %s\n", r.SourceBranch)
	fmt.Printf("Target:   %s\n", r.TargetBranch)
	fmt.Printf("Base:     %s\n\n", r.MergeBase[:8])

	levelColor.Printf("Risk Score: %.2f (%s)\n\n", r.Score, strings.ToUpper(r.Level))

	fmt.Printf("Changes:\n")
	fmt.Printf("  Lines Added:    %d\n", r.LinesAdded)
	fmt.Printf("  Lines Deleted:  %d\n", r.LinesDeleted)
	fmt.Printf("  Files Modified: %d\n", r.FilesModified)
	fmt.Printf("  Commits:        %d\n\n", r.Commits)

	fmt.Printf("Risk Factors:\n")
	for name, score := range r.Factors {
		fmt.Printf("  %-15s %.3f\n", name+":", score)
	}
}
