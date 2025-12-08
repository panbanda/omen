package main

import (
	"bytes"
	"context"
	"fmt"
	"strings"

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
	contextCmd.Flags().Int("max-tokens", 0, "Maximum tokens for LLM context window (0 = unlimited)")
	contextCmd.Flags().Int("max-files", 0, "Maximum files to include in file list (0 = unlimited)")

	rootCmd.AddCommand(contextCmd)
}

// tokenBudget tracks token usage for context window limiting.
type tokenBudget struct {
	maxTokens  int
	usedTokens int
	sections   []string
}

// newTokenBudget creates a token budget tracker.
func newTokenBudget(maxTokens int) *tokenBudget {
	return &tokenBudget{
		maxTokens: maxTokens,
		sections:  make([]string, 0),
	}
}

// estimateTokens estimates tokens from text (rough: ~4 chars per token).
func estimateTokens(text string) int {
	return (len(text) + 3) / 4
}

// hasRoom returns true if there's room for more content.
func (tb *tokenBudget) hasRoom(additionalTokens int) bool {
	if tb.maxTokens == 0 {
		return true
	}
	return tb.usedTokens+additionalTokens <= tb.maxTokens
}

// remaining returns the remaining token budget.
func (tb *tokenBudget) remaining() int {
	if tb.maxTokens == 0 {
		return -1 // unlimited
	}
	return tb.maxTokens - tb.usedTokens
}

// add adds content and returns true if it fits, false if truncated.
func (tb *tokenBudget) add(content string) bool {
	tokens := estimateTokens(content)
	if tb.maxTokens > 0 && tb.usedTokens+tokens > tb.maxTokens {
		return false
	}
	tb.usedTokens += tokens
	return true
}

// trackSection records a section was included.
func (tb *tokenBudget) trackSection(name string) {
	tb.sections = append(tb.sections, name)
}

func runContext(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)
	includeMetrics, _ := cmd.Flags().GetBool("include-metrics")
	includeGraph, _ := cmd.Flags().GetBool("include-graph")
	repoMap, _ := cmd.Flags().GetBool("repo-map")
	topN, _ := cmd.Flags().GetInt("top")
	maxTokens, _ := cmd.Flags().GetInt("max-tokens")
	maxFiles, _ := cmd.Flags().GetInt("max-files")

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

	// Initialize token budget for context window limiting
	budget := newTokenBudget(maxTokens)
	var buf bytes.Buffer
	var truncated []string

	// Helper to write and track tokens
	write := func(s string) bool {
		if !budget.add(s) {
			return false
		}
		buf.WriteString(s)
		return true
	}

	writeln := func(s string) bool {
		return write(s + "\n")
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Output project structure (always included - small overhead)
	writeln("# Project Context")
	writeln("")
	writeln("## Overview")
	writeln(fmt.Sprintf("- **Paths**: %v", paths))
	writeln(fmt.Sprintf("- **Total Files**: %d", len(files)))
	if maxTokens > 0 {
		writeln(fmt.Sprintf("- **Token Budget**: %d", maxTokens))
	}
	writeln("")
	budget.trackSection("Overview")

	// Language distribution (small, always fits)
	writeln("## Language Distribution")
	for lang, langFiles := range langGroups {
		writeln(fmt.Sprintf("- **%s**: %d files", lang, len(langFiles)))
	}
	writeln("")
	budget.trackSection("Language Distribution")

	// File structure - may be truncated for large repos
	writeln("## File Structure")
	fileCount := len(files)
	displayFiles := files

	// Apply max-files limit
	if maxFiles > 0 && fileCount > maxFiles {
		displayFiles = files[:maxFiles]
		truncated = append(truncated, fmt.Sprintf("File list: showing %d of %d files", maxFiles, fileCount))
	}

	// Also check token budget for file list
	filesShown := 0
	for _, f := range displayFiles {
		line := fmt.Sprintf("- %s\n", f)
		if !budget.hasRoom(estimateTokens(line) + 500) { // Reserve 500 tokens for summary
			truncated = append(truncated, fmt.Sprintf("File list: truncated at %d of %d files (token budget)", filesShown, fileCount))
			writeln(fmt.Sprintf("- ... and %d more files", fileCount-filesShown))
			break
		}
		write(line)
		filesShown++
	}
	if filesShown == len(displayFiles) && maxFiles > 0 && fileCount > maxFiles {
		writeln(fmt.Sprintf("- ... and %d more files", fileCount-maxFiles))
	}
	budget.trackSection("File Structure")

	if includeMetrics && budget.hasRoom(500) {
		writeln("")
		writeln("## Complexity Metrics")
		svc := analysis.New()
		cxResult, cxErr := svc.AnalyzeComplexity(context.Background(), files, analysis.ComplexityOptions{})
		if cxErr == nil {
			writeln(fmt.Sprintf("- **Total Functions**: %d", cxResult.Summary.TotalFunctions))
			writeln(fmt.Sprintf("- **Median Cyclomatic (P50)**: %d", cxResult.Summary.P50Cyclomatic))
			writeln(fmt.Sprintf("- **Median Cognitive (P50)**: %d", cxResult.Summary.P50Cognitive))
			writeln(fmt.Sprintf("- **90th Percentile Cyclomatic**: %d", cxResult.Summary.P90Cyclomatic))
			writeln(fmt.Sprintf("- **90th Percentile Cognitive**: %d", cxResult.Summary.P90Cognitive))
			writeln(fmt.Sprintf("- **Max Cyclomatic**: %d", cxResult.Summary.MaxCyclomatic))
			writeln(fmt.Sprintf("- **Max Cognitive**: %d", cxResult.Summary.MaxCognitive))
			budget.trackSection("Complexity Metrics")
		}
	} else if includeMetrics {
		truncated = append(truncated, "Complexity Metrics: skipped (token budget)")
	}

	if includeGraph && budget.hasRoom(1000) {
		writeln("")
		writeln("## Dependency Graph")
		graphSvc := analysis.New()
		graphData, _, graphErr := graphSvc.AnalyzeGraph(context.Background(), files, analysis.GraphOptions{
			Scope: graph.ScopeFile,
		})
		if graphErr == nil {
			// Build graph content to check size
			var graphBuf strings.Builder
			graphBuf.WriteString("```mermaid\n")
			graphBuf.WriteString("graph TD\n")

			nodeCount := 0
			maxNodes := 50
			if maxTokens > 0 {
				// Limit graph nodes based on budget
				maxNodes = budget.remaining() / 50 // ~50 tokens per node
				if maxNodes > 100 {
					maxNodes = 100
				}
				if maxNodes < 10 {
					maxNodes = 10
				}
			}

			for _, node := range graphData.Nodes {
				if nodeCount >= maxNodes {
					graphBuf.WriteString(fmt.Sprintf("    truncated[... and %d more nodes]\n", len(graphData.Nodes)-nodeCount))
					truncated = append(truncated, fmt.Sprintf("Graph: showing %d of %d nodes", nodeCount, len(graphData.Nodes)))
					break
				}
				graphBuf.WriteString(fmt.Sprintf("    %s[%s]\n", sanitizeID(node.ID), node.Name))
				nodeCount++
			}

			edgeCount := 0
			maxEdges := maxNodes * 2
			for _, edge := range graphData.Edges {
				if edgeCount >= maxEdges {
					break
				}
				graphBuf.WriteString(fmt.Sprintf("    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To)))
				edgeCount++
			}
			graphBuf.WriteString("```\n")

			graphContent := graphBuf.String()
			if budget.add(graphContent) {
				buf.WriteString(graphContent)
				budget.trackSection("Dependency Graph")
			} else {
				truncated = append(truncated, "Dependency Graph: skipped (token budget)")
			}
		}
	} else if includeGraph {
		truncated = append(truncated, "Dependency Graph: skipped (token budget)")
	}

	if repoMap && budget.hasRoom(500) {
		writeln("")
		writeln("## Repository Map")

		spinner := progress.NewSpinner("Generating repo map...")
		rmSvc := analysis.New()
		rm, rmErr := rmSvc.AnalyzeRepoMap(context.Background(), files, analysis.RepoMapOptions{Top: topN})
		spinner.FinishSuccess()

		if rmErr == nil {
			// Adjust top N based on remaining budget
			effectiveTopN := topN
			if maxTokens > 0 {
				tokensPerSymbol := 80
				maxSymbols := budget.remaining() / tokensPerSymbol
				if maxSymbols < effectiveTopN {
					effectiveTopN = maxSymbols
					if effectiveTopN < 10 {
						effectiveTopN = 10
					}
					truncated = append(truncated, fmt.Sprintf("Repository Map: showing %d of %d requested symbols", effectiveTopN, topN))
				}
			}

			topSymbols := rm.TopN(effectiveTopN)
			writeln(fmt.Sprintf("Top %d symbols by PageRank:\n", len(topSymbols)))
			writeln("| Symbol | Kind | File | Line | PageRank |")
			writeln("|--------|------|------|------|----------|")
			for _, s := range topSymbols {
				line := fmt.Sprintf("| %s | %s | %s | %d | %.4f |\n",
					s.Name, s.Kind, s.File, s.Line, s.PageRank)
				if !budget.hasRoom(estimateTokens(line) + 100) {
					writeln("| ... | ... | ... | ... | ... |")
					break
				}
				write(line)
			}
			writeln("")
			writeln(fmt.Sprintf("- **Total Symbols**: %d", rm.Summary.TotalSymbols))
			writeln(fmt.Sprintf("- **Total Files**: %d", rm.Summary.TotalFiles))
			writeln(fmt.Sprintf("- **Max PageRank**: %.4f", rm.Summary.MaxPageRank))
			budget.trackSection("Repository Map")
		}
	} else if repoMap {
		truncated = append(truncated, "Repository Map: skipped (token budget)")
	}

	// Add truncation summary if anything was truncated
	if len(truncated) > 0 {
		writeln("")
		writeln("## Context Summary")
		if maxTokens > 0 {
			writeln(fmt.Sprintf("- **Token Budget**: %d", maxTokens))
			writeln(fmt.Sprintf("- **Tokens Used**: ~%d", budget.usedTokens))
		}
		if maxFiles > 0 {
			writeln(fmt.Sprintf("- **File Limit**: %d", maxFiles))
		}
		writeln(fmt.Sprintf("- **Sections**: %s", strings.Join(budget.sections, ", ")))
		writeln("")
		writeln("**Truncations:**")
		for _, t := range truncated {
			writeln(fmt.Sprintf("- %s", t))
		}
	}

	// Output the buffered content
	fmt.Print(buf.String())

	return nil
}
