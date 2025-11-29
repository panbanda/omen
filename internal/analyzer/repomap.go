package analyzer

import (
	"time"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
)

// RepoMapAnalyzer generates a PageRank-ranked map of repository symbols.
type RepoMapAnalyzer struct {
	graphAnalyzer *GraphAnalyzer
}

// NewRepoMapAnalyzer creates a new repo map analyzer.
func NewRepoMapAnalyzer() *RepoMapAnalyzer {
	return &RepoMapAnalyzer{
		graphAnalyzer: NewGraphAnalyzer(ScopeFunction),
	}
}

// AnalyzeProject generates a repo map for the given files.
func (a *RepoMapAnalyzer) AnalyzeProject(files []string) (*models.RepoMap, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress generates a repo map with progress callback.
func (a *RepoMapAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress fileproc.ProgressFunc) (*models.RepoMap, error) {
	// Build the dependency graph at function scope
	graph, err := a.graphAnalyzer.AnalyzeProjectWithProgress(files, onProgress)
	if err != nil {
		return nil, err
	}

	// Calculate PageRank only - much faster than full metrics
	metrics := a.graphAnalyzer.CalculatePageRankOnly(graph)

	// Build repo map from graph nodes and metrics
	repoMap := &models.RepoMap{
		GeneratedAt: time.Now().UTC(),
		Symbols:     make([]models.Symbol, 0, len(graph.Nodes)),
	}

	// Build lookup for node metrics
	metricsByID := make(map[string]*models.NodeMetric)
	for i := range metrics.NodeMetrics {
		m := &metrics.NodeMetrics[i]
		metricsByID[m.NodeID] = m
	}

	// Convert graph nodes to symbols
	for _, node := range graph.Nodes {
		symbol := models.Symbol{
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
func generateSignature(node models.GraphNode) string {
	switch node.Type {
	case models.NodeFunction:
		return "func " + node.Name + "()"
	case models.NodeClass:
		return "class " + node.Name
	case models.NodeModule:
		return "module " + node.Name
	case models.NodeFile:
		return node.Name
	default:
		return node.Name
	}
}

// Close releases resources.
func (a *RepoMapAnalyzer) Close() {
	if a.graphAnalyzer != nil {
		a.graphAnalyzer.Close()
	}
}
