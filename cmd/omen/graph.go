package main

import (
	"context"
	"fmt"
	"sort"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/spf13/cobra"
)

var graphCmd = &cobra.Command{
	Use:     "graph [path...]",
	Aliases: []string{"dag"},
	Short:   "Generate dependency graph (Mermaid output)",
	RunE:    runGraph,
}

func init() {
	graphCmd.Flags().String("scope", "module", "Scope: file, function, module, package")
	graphCmd.Flags().Bool("metrics", false, "Include PageRank and centrality metrics")

	analyzeCmd.AddCommand(graphCmd)
}

func runGraph(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	scope, _ := cmd.Flags().GetString("scope")
	includeMetrics, _ := cmd.Flags().GetBool("metrics")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Building dependency graph...", len(scanResult.Files))
	svc := analysis.New()
	graphResult, metrics, err := svc.AnalyzeGraph(context.Background(), scanResult.Files, analysis.GraphOptions{
		Scope:          graph.Scope(scope),
		IncludeMetrics: includeMetrics,
		OnProgress:     tracker.Tick,
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

	// For JSON/TOON, output structured data
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		if includeMetrics && metrics != nil {
			return formatter.Output(struct {
				Graph   *graph.DependencyGraph `json:"graph" toon:"graph"`
				Metrics *graph.Metrics         `json:"metrics" toon:"metrics"`
			}{graphResult, metrics})
		}
		return formatter.Output(graphResult)
	}

	// Generate Mermaid diagram for text/markdown
	fmt.Fprintln(formatter.Writer(), "```mermaid")
	fmt.Fprintln(formatter.Writer(), "graph TD")
	for _, node := range graphResult.Nodes {
		fmt.Fprintf(formatter.Writer(), "    %s[%s]\n", sanitizeID(node.ID), node.Name)
	}
	for _, edge := range graphResult.Edges {
		fmt.Fprintf(formatter.Writer(), "    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
	}
	fmt.Fprintln(formatter.Writer(), "```")

	if includeMetrics && metrics != nil {
		fmt.Fprintln(formatter.Writer())
		if formatter.Colored() {
			color.Cyan("Graph Metrics:")
		} else {
			fmt.Fprintln(formatter.Writer(), "Graph Metrics:")
		}
		fmt.Fprintf(formatter.Writer(), "  Nodes: %d\n", metrics.Summary.TotalNodes)
		fmt.Fprintf(formatter.Writer(), "  Edges: %d\n", metrics.Summary.TotalEdges)
		fmt.Fprintf(formatter.Writer(), "  Avg Degree: %.2f\n", metrics.Summary.AvgDegree)
		fmt.Fprintf(formatter.Writer(), "  Density: %.4f\n", metrics.Summary.Density)

		if len(metrics.NodeMetrics) > 0 {
			fmt.Fprintln(formatter.Writer())
			if formatter.Colored() {
				color.Cyan("Top Nodes by PageRank:")
			} else {
				fmt.Fprintln(formatter.Writer(), "Top Nodes by PageRank:")
			}
			sort.Slice(metrics.NodeMetrics, func(i, j int) bool {
				return metrics.NodeMetrics[i].PageRank > metrics.NodeMetrics[j].PageRank
			})
			for i, nm := range metrics.NodeMetrics {
				if i >= 5 {
					break
				}
				fmt.Fprintf(formatter.Writer(), "  %s: %.4f (in: %d, out: %d)\n",
					nm.Name, nm.PageRank, nm.InDegree, nm.OutDegree)
			}
		}
	}

	return nil
}
