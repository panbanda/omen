package main

import (
	"context"
	"fmt"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/spf13/cobra"
)

var contextCmd = &cobra.Command{
	Use:     "context [path...]",
	Aliases: []string{"ctx"},
	Short:   "Generate deep context for LLM consumption",
	RunE:    runContext,
}

func init() {
	contextCmd.Flags().Bool("include-metrics", false, "Include complexity and quality metrics")
	contextCmd.Flags().Bool("include-graph", false, "Include dependency graph")
	contextCmd.Flags().Bool("repo-map", false, "Generate PageRank-ranked symbol map")
	contextCmd.Flags().Int("top", 50, "Number of top symbols to include in repo map")

	rootCmd.AddCommand(contextCmd)
}

func runContext(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	includeMetrics, _ := cmd.Flags().GetBool("include-metrics")
	includeGraph, _ := cmd.Flags().GetBool("include-graph")
	repoMap, _ := cmd.Flags().GetBool("repo-map")
	topN, _ := cmd.Flags().GetInt("top")

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
	fmt.Printf("## Overview\n")
	fmt.Printf("- **Paths**: %v\n", paths)
	fmt.Printf("- **Total Files**: %d\n", len(files))
	fmt.Println()

	fmt.Println("## Language Distribution")
	for lang, langFiles := range langGroups {
		fmt.Printf("- **%s**: %d files\n", lang, len(langFiles))
	}
	fmt.Println()

	fmt.Println("## File Structure")
	for _, f := range files {
		fmt.Printf("- %s\n", f)
	}

	if includeMetrics {
		fmt.Println()
		fmt.Println("## Complexity Metrics")
		svc := analysis.New()
		cxResult, cxErr := svc.AnalyzeComplexity(context.Background(), files, analysis.ComplexityOptions{})
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
		graphSvc := analysis.New()
		graphData, _, graphErr := graphSvc.AnalyzeGraph(context.Background(), files, analysis.GraphOptions{
			Scope: graph.ScopeFile,
		})
		if graphErr == nil {
			fmt.Println("```mermaid")
			fmt.Println("graph TD")
			for _, node := range graphData.Nodes {
				fmt.Printf("    %s[%s]\n", sanitizeID(node.ID), node.Name)
			}
			for _, edge := range graphData.Edges {
				fmt.Printf("    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
			}
			fmt.Println("```")
		}
	}

	if repoMap {
		fmt.Println()
		fmt.Println("## Repository Map")

		spinner := progress.NewSpinner("Generating repo map...")
		rmSvc := analysis.New()
		rm, rmErr := rmSvc.AnalyzeRepoMap(context.Background(), files, analysis.RepoMapOptions{Top: topN})
		spinner.FinishSuccess()

		if rmErr == nil {
			// Check if we should output using formatter
			formatStr := getFormat(cmd)
			if formatStr != "" && formatStr != "text" {
				// Use formatter for non-text output
				topSymbols := rm.TopN(topN)

				var rows [][]string
				for _, s := range topSymbols {
					rows = append(rows, []string{
						s.Name,
						s.Kind,
						s.File,
						fmt.Sprintf("%d", s.Line),
						fmt.Sprintf("%.4f", s.PageRank),
						fmt.Sprintf("%d", s.InDegree),
						fmt.Sprintf("%d", s.OutDegree),
					})
				}

				table := output.NewTable(
					fmt.Sprintf("Repository Map (Top %d Symbols by PageRank)", topN),
					[]string{"Name", "Kind", "File", "Line", "PageRank", "In-Degree", "Out-Degree"},
					rows,
					[]string{
						fmt.Sprintf("Total Symbols: %d", rm.Summary.TotalSymbols),
						fmt.Sprintf("Total Files: %d", rm.Summary.TotalFiles),
						fmt.Sprintf("Avg Connections: %.1f", rm.Summary.AvgConnections),
					},
					rm,
				)

				if err := formatter.Output(table); err != nil {
					return err
				}
			} else {
				// Text/markdown output
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
	}

	return nil
}
