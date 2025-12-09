package main

import (
	"context"
	"fmt"
	"path/filepath"

	"github.com/fatih/color"
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

	// If --focus is provided, run focused context
	if focus != "" {
		return runFocusedContext(cmd, focus, paths)
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
	fmt.Println("# Project Context")
	fmt.Println()
	fmt.Println("## Overview")
	fmt.Printf("- **Paths**: %v\n", paths)
	fmt.Printf("- **Total Files**: %d\n", len(files))
	fmt.Println()

	// Language distribution
	fmt.Println("## Language Distribution")
	for lang, langFiles := range langGroups {
		fmt.Printf("- **%s**: %d files\n", lang, len(langFiles))
	}
	fmt.Println()

	// File structure sorted by hotspot score (most problematic first)
	fmt.Println("## File Structure")
	fmt.Println("*Sorted by hotspot score (churn + complexity)*")
	fmt.Println()

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
				fmt.Printf("- ... and %d more files\n", len(rankedFiles)-maxFiles)
				break
			}
			if rf.Score > 0 {
				fmt.Printf("- %s (%.0f%%)\n", rf.Path, rf.Score*100)
			} else {
				fmt.Printf("- %s\n", rf.Path)
			}
		}
	} else {
		// Fallback: unsorted list (no git or error)
		if sortErr != nil && verbose {
			color.Yellow("Note: hotspot sorting unavailable (%v), showing unsorted file list", sortErr)
		}
		for i, f := range files {
			if i >= maxFiles {
				fmt.Printf("- ... and %d more files\n", len(files)-maxFiles)
				break
			}
			fmt.Printf("- %s\n", f)
		}
	}

	if includeMetrics {
		fmt.Println()
		fmt.Println("## Complexity Metrics")
		cxResult, cxErr := analysisSvc.AnalyzeComplexity(context.Background(), files, analysis.ComplexityOptions{})
		if cxErr == nil {
			fmt.Printf("- **Total Functions**: %d\n", cxResult.Summary.TotalFunctions)
			fmt.Printf("- **Median Cyclomatic (P50)**: %d\n", cxResult.Summary.P50Cyclomatic)
			fmt.Printf("- **Median Cognitive (P50)**: %d\n", cxResult.Summary.P50Cognitive)
			fmt.Printf("- **90th Percentile Cyclomatic**: %d\n", cxResult.Summary.P90Cyclomatic)
			fmt.Printf("- **90th Percentile Cognitive**: %d\n", cxResult.Summary.P90Cognitive)
			fmt.Printf("- **Max Cyclomatic**: %d\n", cxResult.Summary.MaxCyclomatic)
			fmt.Printf("- **Max Cognitive**: %d\n", cxResult.Summary.MaxCognitive)
		}
	}

	if includeGraph {
		fmt.Println()
		fmt.Println("## Dependency Graph")
		graphData, _, graphErr := analysisSvc.AnalyzeGraph(context.Background(), files, analysis.GraphOptions{
			Scope: graph.ScopeFile,
		})
		if graphErr == nil {
			fmt.Println("```mermaid")
			fmt.Println("graph TD")

			maxNodes := defaultMaxGraphNodes
			if fullOutput {
				maxNodes = len(graphData.Nodes)
			}

			for i, node := range graphData.Nodes {
				if i >= maxNodes {
					fmt.Printf("    truncated[... and %d more nodes]\n", len(graphData.Nodes)-maxNodes)
					break
				}
				fmt.Printf("    %s[%s]\n", sanitizeID(node.ID), node.Name)
			}

			maxEdges := maxNodes * 2
			for i, edge := range graphData.Edges {
				if i >= maxEdges {
					break
				}
				fmt.Printf("    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
			}
			fmt.Println("```")
		}
	}

	if repoMap {
		fmt.Println()
		fmt.Println("## Repository Map")

		spinner := progress.NewSpinner("Generating repo map...")
		rm, rmErr := analysisSvc.AnalyzeRepoMap(context.Background(), files, analysis.RepoMapOptions{Top: topN})
		spinner.FinishSuccess()

		if rmErr == nil {
			topSymbols := rm.TopN(topN)
			fmt.Printf("Top %d symbols by PageRank:\n\n", len(topSymbols))
			fmt.Println("| Symbol | Kind | File | Line | PageRank |")
			fmt.Println("|--------|------|------|------|----------|")
			for _, s := range topSymbols {
				fmt.Printf("| %s | %s | %s | %d | %.4f |\n",
					s.Name, s.Kind, s.File, s.Line, s.PageRank)
			}
			fmt.Println()
			fmt.Printf("- **Total Symbols**: %d\n", rm.Summary.TotalSymbols)
			fmt.Printf("- **Total Files**: %d\n", rm.Summary.TotalFiles)
			fmt.Printf("- **Max PageRank**: %.4f\n", rm.Summary.MaxPageRank)
		}
	}

	return nil
}

func runFocusedContext(cmd *cobra.Command, focus string, paths []string) error {
	baseDir := "."
	if len(paths) > 0 {
		baseDir = paths[0]
	}

	// Generate repo map for symbol resolution
	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	analysisSvc := analysis.New()

	// Generate repo map for symbol lookup
	var repoMapResult *repomap.Map
	if len(scanResult.Files) > 0 {
		repoMapResult, _ = analysisSvc.AnalyzeRepoMap(context.Background(), scanResult.Files, analysis.RepoMapOptions{})
	}

	result, err := analysisSvc.FocusedContext(context.Background(), analysis.FocusedContextOptions{
		Focus:   focus,
		BaseDir: baseDir,
		RepoMap: repoMapResult,
	})

	// Handle ambiguous match
	if err != nil && result != nil && len(result.Candidates) > 0 {
		fmt.Println("# Ambiguous Match")
		fmt.Println()
		fmt.Printf("Multiple matches found for '%s'. Please be more specific:\n\n", focus)
		for _, c := range result.Candidates {
			if c.Path != "" {
				fmt.Printf("- %s\n", c.Path)
			} else {
				fmt.Printf("- %s (%s) at %s:%d\n", c.Name, c.Kind, c.File, c.Line)
			}
		}
		return nil
	}

	if err != nil {
		return err
	}

	// Output focused context
	fmt.Println("# Focused Context")
	fmt.Println()

	// Target information
	fmt.Println("## Target")
	if result.Target.Type == "file" {
		fmt.Printf("- **Type**: file\n")
		fmt.Printf("- **Path**: %s\n", result.Target.Path)
	} else if result.Target.Type == "symbol" && result.Target.Symbol != nil {
		fmt.Printf("- **Type**: symbol\n")
		fmt.Printf("- **Name**: %s\n", result.Target.Symbol.Name)
		fmt.Printf("- **Kind**: %s\n", result.Target.Symbol.Kind)
		fmt.Printf("- **File**: %s\n", result.Target.Symbol.File)
		fmt.Printf("- **Line**: %d\n", result.Target.Symbol.Line)
	}
	fmt.Println()

	// Complexity
	if result.Complexity != nil {
		fmt.Println("## Complexity")
		fmt.Printf("- **Cyclomatic Total**: %d\n", result.Complexity.CyclomaticTotal)
		fmt.Printf("- **Cognitive Total**: %d\n", result.Complexity.CognitiveTotal)
		if len(result.Complexity.TopFunctions) > 0 {
			fmt.Println()
			fmt.Println("### Functions")
			fmt.Println("| Name | Line | Cyclomatic | Cognitive |")
			fmt.Println("|------|------|------------|-----------|")
			for _, fn := range result.Complexity.TopFunctions {
				fmt.Printf("| %s | %d | %d | %d |\n", fn.Name, fn.Line, fn.Cyclomatic, fn.Cognitive)
			}
		}
		fmt.Println()
	}

	// SATD markers
	if len(result.SATD) > 0 {
		fmt.Println("## Technical Debt")
		fmt.Println("| Line | Type | Severity | Description |")
		fmt.Println("|------|------|----------|-------------|")
		for _, item := range result.SATD {
			fmt.Printf("| %d | %s | %s | %s |\n", item.Line, item.Type, item.Severity, item.Content)
		}
		fmt.Println()
	}

	return nil
}
