package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/locator"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/repomap"
	"github.com/spf13/cobra"
)

// Default limits for context output to keep it LLM-friendly
const (
	defaultMaxFiles      = 100
	defaultMaxGraphNodes = 50
	defaultMaxSymbols    = 50
)

var contextCmd = &cobra.Command{
	Use:     "context [path...]",
	Aliases: []string{"ctx"},
	Short:   "Generate deep context for LLM consumption",
	RunE:    runContext,
}

func init() {
	contextCmd.Flags().String("focus", "", "Focus on a specific file or symbol (file path, glob pattern, basename, or symbol name)")
	contextCmd.Flags().Bool("include-metrics", false, "Include complexity and quality metrics")
	contextCmd.Flags().Bool("include-graph", false, "Include dependency graph")
	contextCmd.Flags().Bool("repo-map", false, "Generate PageRank-ranked symbol map")
	contextCmd.Flags().Int("top", defaultMaxSymbols, "Number of top symbols to include in repo map")
	contextCmd.Flags().Bool("full", false, "Include all files without limits (use analyzers directly for detailed output)")
	contextCmd.Flags().Bool("tokens", false, "Show estimated token count and budget usage")
	contextCmd.Flags().Int("budget", 128000, "Token budget for estimation (default: 128k)")

	rootCmd.AddCommand(contextCmd)
}

func runContext(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	focus, _ := cmd.Flags().GetString("focus")
	includeMetrics, _ := cmd.Flags().GetBool("include-metrics")
	includeGraph, _ := cmd.Flags().GetBool("include-graph")
	repoMap, _ := cmd.Flags().GetBool("repo-map")
	topN, _ := cmd.Flags().GetInt("top")
	fullOutput, _ := cmd.Flags().GetBool("full")
	showTokens, _ := cmd.Flags().GetBool("tokens")
	tokenBudget, _ := cmd.Flags().GetInt("budget")

	// Set up output capture for token counting
	var buf bytes.Buffer
	var writer io.Writer = os.Stdout
	if showTokens {
		writer = io.MultiWriter(os.Stdout, &buf)
	}
	printFn := func(format string, a ...interface{}) {
		fmt.Fprintf(writer, format, a...)
	}
	printLn := func(a ...interface{}) {
		fmt.Fprintln(writer, a...)
	}

	// If --focus is provided, run focused context
	if focus != "" {
		return runFocusedContext(cmd, focus, paths, showTokens, tokenBudget)
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

	files := scanResult.Files
	langGroups := scanResult.LanguageGroups

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Output project structure
	printLn("# Project Context")
	printLn()
	printLn("## Overview")
	printFn("- **Paths**: %v\n", paths)
	printFn("- **Total Files**: %d\n", len(files))
	printLn()

	// Language distribution
	printLn("## Language Distribution")
	for lang, langFiles := range langGroups {
		printFn("- **%s**: %d files\n", lang, len(langFiles))
	}
	printLn()

	// File structure sorted by hotspot score (most problematic first)
	printLn("## File Structure")
	printLn("*Sorted by hotspot score (churn + complexity)*")
	printLn()

	maxFiles := defaultMaxFiles
	if fullOutput {
		maxFiles = len(files)
	}

	// Try to sort by hotspot score (requires git repo)
	// Use ScanPathsForGit to find the git root
	gitResult, gitErr := scanSvc.ScanPathsForGit(paths, false)
	repoRoot := ""
	if gitErr == nil && gitResult.RepoRoot != "" {
		repoRoot = gitResult.RepoRoot
	} else {
		// Fallback to cwd if scanning didn't find repo root
		repoRoot, _ = filepath.Abs(".")
	}

	analysisSvc := analysis.New()
	rankedFiles, sortErr := analysisSvc.SortFilesByHotspot(context.Background(), repoRoot, files, analysis.HotspotOptions{})

	if sortErr == nil && len(rankedFiles) > 0 {
		// Display sorted files with scores
		for i, rf := range rankedFiles {
			if i >= maxFiles {
				printFn("- ... and %d more files\n", len(rankedFiles)-maxFiles)
				break
			}
			if rf.Score > 0 {
				printFn("- %s (%.0f%%)\n", rf.Path, rf.Score*100)
			} else {
				printFn("- %s\n", rf.Path)
			}
		}
	} else {
		// Fallback: unsorted list (no git or error)
		if sortErr != nil && verbose {
			color.Yellow("Note: hotspot sorting unavailable (%v), showing unsorted file list", sortErr)
		}
		for i, f := range files {
			if i >= maxFiles {
				printFn("- ... and %d more files\n", len(files)-maxFiles)
				break
			}
			printFn("- %s\n", f)
		}
	}

	if includeMetrics {
		printLn()
		printLn("## Complexity Metrics")
		cxResult, cxErr := analysisSvc.AnalyzeComplexity(context.Background(), files, analysis.ComplexityOptions{})
		if cxErr == nil {
			printFn("- **Total Functions**: %d\n", cxResult.Summary.TotalFunctions)
			printFn("- **Median Cyclomatic (P50)**: %d\n", cxResult.Summary.P50Cyclomatic)
			printFn("- **Median Cognitive (P50)**: %d\n", cxResult.Summary.P50Cognitive)
			printFn("- **90th Percentile Cyclomatic**: %d\n", cxResult.Summary.P90Cyclomatic)
			printFn("- **90th Percentile Cognitive**: %d\n", cxResult.Summary.P90Cognitive)
			printFn("- **Max Cyclomatic**: %d\n", cxResult.Summary.MaxCyclomatic)
			printFn("- **Max Cognitive**: %d\n", cxResult.Summary.MaxCognitive)
		}
	}

	if includeGraph {
		printLn()
		printLn("## Dependency Graph")
		graphData, _, graphErr := analysisSvc.AnalyzeGraph(context.Background(), files, analysis.GraphOptions{
			Scope: graph.ScopeFile,
		})
		if graphErr == nil {
			printLn("```mermaid")
			printLn("graph TD")

			maxNodes := defaultMaxGraphNodes
			if fullOutput {
				maxNodes = len(graphData.Nodes)
			}

			for i, node := range graphData.Nodes {
				if i >= maxNodes {
					printFn("    truncated[... and %d more nodes]\n", len(graphData.Nodes)-maxNodes)
					break
				}
				printFn("    %s[%s]\n", sanitizeID(node.ID), node.Name)
			}

			maxEdges := maxNodes * 2
			for i, edge := range graphData.Edges {
				if i >= maxEdges {
					break
				}
				printFn("    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
			}
			printLn("```")
		}
	}

	if repoMap {
		printLn()
		printLn("## Repository Map")

		spinner := progress.NewSpinner("Generating repo map...")
		rm, rmErr := analysisSvc.AnalyzeRepoMap(context.Background(), files, analysis.RepoMapOptions{Top: topN})
		spinner.FinishSuccess()

		if rmErr == nil {
			topSymbols := rm.TopN(topN)
			printFn("Top %d symbols by PageRank:\n\n", len(topSymbols))
			printLn("| Symbol | Kind | File | Line | PageRank |")
			printLn("|--------|------|------|------|----------|")
			for _, s := range topSymbols {
				printFn("| %s | %s | %s | %d | %.4f |\n",
					s.Name, s.Kind, s.File, s.Line, s.PageRank)
			}
			printLn()
			printFn("- **Total Symbols**: %d\n", rm.Summary.TotalSymbols)
			printFn("- **Total Files**: %d\n", rm.Summary.TotalFiles)
			printFn("- **Max PageRank**: %.4f\n", rm.Summary.MaxPageRank)
		}
	}

	// Show token budget info if requested
	if showTokens {
		info := output.GetTokenBudgetInfo(buf.String(), tokenBudget)
		fmt.Println()
		fmt.Println("---")
		fmt.Printf("**Token Estimate**: %s / %s (%.1f%% of budget)\n",
			output.FormatTokenCount(info.Tokens),
			info.BudgetLabel,
			info.UsagePercent)
	}

	return nil
}

func runFocusedContext(cmd *cobra.Command, focus string, paths []string, showTokens bool, tokenBudget int) error {
	// Set up output capture for token counting
	var buf bytes.Buffer
	var writer io.Writer = os.Stdout
	if showTokens {
		writer = io.MultiWriter(os.Stdout, &buf)
	}
	printFn := func(format string, a ...interface{}) {
		fmt.Fprintf(writer, format, a...)
	}
	printLn := func(a ...interface{}) {
		fmt.Fprintln(writer, a...)
	}

	baseDir := "."
	if len(paths) > 0 {
		baseDir = paths[0]
	}

	analysisSvc := analysis.New()

	// Try without repo map first (exact path, glob, basename)
	result, err := analysisSvc.FocusedContext(context.Background(), analysis.FocusedContextOptions{
		Focus:   focus,
		BaseDir: baseDir,
	})

	// If not found, try with repo map for symbol lookup
	if errors.Is(err, locator.ErrNotFound) {
		scanSvc := scannerSvc.New()
		scanResult, scanErr := scanSvc.ScanPaths(paths)
		if scanErr == nil && len(scanResult.Files) > 0 {
			var repoMapResult *repomap.Map
			repoMapResult, _ = analysisSvc.AnalyzeRepoMap(context.Background(), scanResult.Files, analysis.RepoMapOptions{})
			if repoMapResult != nil {
				result, err = analysisSvc.FocusedContext(context.Background(), analysis.FocusedContextOptions{
					Focus:   focus,
					BaseDir: baseDir,
					RepoMap: repoMapResult,
				})
			}
		}
	}

	// Handle ambiguous match
	if err != nil && result != nil && len(result.Candidates) > 0 {
		printLn("# Ambiguous Match")
		printLn()
		printFn("Multiple matches found for '%s'. Please be more specific:\n\n", focus)
		for _, c := range result.Candidates {
			if c.Path != "" {
				printFn("- %s\n", c.Path)
			} else {
				printFn("- %s (%s) at %s:%d\n", c.Name, c.Kind, c.File, c.Line)
			}
		}
		return nil
	}

	if err != nil {
		return err
	}

	// Output focused context
	printLn("# Focused Context")
	printLn()

	// Target information
	printLn("## Target")
	if result.Target.Type == "file" {
		printFn("- **Type**: file\n")
		printFn("- **Path**: %s\n", result.Target.Path)
	} else if result.Target.Type == "symbol" && result.Target.Symbol != nil {
		printFn("- **Type**: symbol\n")
		printFn("- **Name**: %s\n", result.Target.Symbol.Name)
		printFn("- **Kind**: %s\n", result.Target.Symbol.Kind)
		printFn("- **File**: %s\n", result.Target.Symbol.File)
		printFn("- **Line**: %d\n", result.Target.Symbol.Line)
	}
	printLn()

	// Complexity
	if result.Complexity != nil {
		printLn("## Complexity")
		printFn("- **Cyclomatic Total**: %d\n", result.Complexity.CyclomaticTotal)
		printFn("- **Cognitive Total**: %d\n", result.Complexity.CognitiveTotal)
		if len(result.Complexity.TopFunctions) > 0 {
			printLn()
			printLn("### Functions")
			printLn("| Name | Line | Cyclomatic | Cognitive |")
			printLn("|------|------|------------|-----------|")
			for _, fn := range result.Complexity.TopFunctions {
				printFn("| %s | %d | %d | %d |\n", fn.Name, fn.Line, fn.Cyclomatic, fn.Cognitive)
			}
		}
		printLn()
	}

	// SATD markers
	if len(result.SATD) > 0 {
		printLn("## Technical Debt")
		printLn("| Line | Type | Severity | Description |")
		printLn("|------|------|----------|-------------|")
		for _, item := range result.SATD {
			printFn("| %d | %s | %s | %s |\n", item.Line, item.Type, item.Severity, item.Content)
		}
		printLn()
	}

	// Show token budget info if requested
	if showTokens {
		info := output.GetTokenBudgetInfo(buf.String(), tokenBudget)
		fmt.Println()
		fmt.Println("---")
		fmt.Printf("**Token Estimate**: %s / %s (%.1f%% of budget)\n",
			output.FormatTokenCount(info.Tokens),
			info.BudgetLabel,
			info.UsagePercent)
	}

	return nil
}
