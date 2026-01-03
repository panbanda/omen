package repomap

import (
	"context"
	"time"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/source"
)

// Ensure Analyzer implements analyzer.FileAnalyzer.
var _ analyzer.FileAnalyzer[*Map] = (*Analyzer)(nil)

// Analyzer generates a PageRank-ranked map of repository symbols.
type Analyzer struct {
	graphAnalyzer *graph.Analyzer
	maxFileSize   int64
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// New creates a new repo map analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	// Create graph analyzer with same maxFileSize setting
	a.graphAnalyzer = graph.New(
		graph.WithScope(graph.ScopeFunction),
		graph.WithMaxFileSize(a.maxFileSize),
	)
	return a
}

// Analyze generates a repo map for the given files.
// Progress can be tracked by passing a context with analyzer.WithProgress.
func (a *Analyzer) Analyze(ctx context.Context, files []string) (*Map, error) {
	// Build the dependency graph at function scope
	depGraph, err := a.graphAnalyzer.Analyze(ctx, files, source.NewFilesystem())
	if err != nil {
		return nil, err
	}

	// Calculate PageRank only - much faster than full metrics
	metrics := a.graphAnalyzer.CalculatePageRankOnly(depGraph)

	// Build repo map from graph nodes and metrics
	repoMap := &Map{
		GeneratedAt: time.Now().UTC(),
		Symbols:     make([]Symbol, 0, len(depGraph.Nodes)),
	}

	// Build lookup for node metrics
	metricsByID := make(map[string]*graph.NodeMetric)
	for i := range metrics.NodeMetrics {
		m := &metrics.NodeMetrics[i]
		metricsByID[m.NodeID] = m
	}

	// Convert graph nodes to symbols
	for _, node := range depGraph.Nodes {
		symbol := Symbol{
			Name: node.Name,
			Kind: string(node.Type),
			File: node.File,
			Line: int(node.Line),
		}

		// Add metrics if available
		if m, ok := metricsByID[node.ID]; ok {
			symbol.PageRank = m.PageRank
			symbol.InDegree = m.InDegree
			symbol.OutDegree = m.OutDegree
		}

		// Generate signature (simplified: just use name for now)
		symbol.Signature = generateSignature(node)

		repoMap.Symbols = append(repoMap.Symbols, symbol)
	}

	repoMap.SortByPageRank()
	repoMap.CalculateSummary()

	return repoMap, nil
}

// generateSignature creates a signature string for a node.
func generateSignature(node graph.Node) string {
	// Use extracted signature from attributes if available
	if node.Attributes != nil {
		if sig, ok := node.Attributes["signature"]; ok && sig != "" {
			return sig
		}
	}

	// Fallback to simple signature
	switch node.Type {
	case graph.NodeFunction:
		return "func " + node.Name + "()"
	case graph.NodeClass:
		return "class " + node.Name
	case graph.NodeModule:
		return "module " + node.Name
	case graph.NodeFile:
		return node.Name
	default:
		return node.Name
	}
}

// Close releases resources.
func (a *Analyzer) Close() {
	if a.graphAnalyzer != nil {
		a.graphAnalyzer.Close()
	}
}
