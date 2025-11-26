package analyzer

import (
	"sync"

	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// GraphAnalyzer builds dependency graphs from source code.
type GraphAnalyzer struct {
	parser *parser.Parser
	scope  GraphScope
}

// GraphScope determines the granularity of graph nodes.
type GraphScope string

const (
	ScopeFile     GraphScope = "file"
	ScopeFunction GraphScope = "function"
	ScopeModule   GraphScope = "module"
	ScopePackage  GraphScope = "package"
)

// NewGraphAnalyzer creates a new graph analyzer.
func NewGraphAnalyzer(scope GraphScope) *GraphAnalyzer {
	return &GraphAnalyzer{
		parser: parser.New(),
		scope:  scope,
	}
}

// fileGraphData holds parsed graph data for a single file.
type fileGraphData struct {
	nodes   []models.GraphNode
	imports []string
	calls   map[string][]string // nodeID -> called functions
}

// AnalyzeProject builds a dependency graph for a project.
func (a *GraphAnalyzer) AnalyzeProject(files []string) (*models.DependencyGraph, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress builds a dependency graph with optional progress callback.
func (a *GraphAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress ProgressFunc) (*models.DependencyGraph, error) {
	// Parse all files in parallel, extracting nodes and edge data
	fileData := MapFilesWithProgress(files, func(psr *parser.Parser, path string) (fileGraphData, error) {
		return a.analyzeFileGraph(psr, path)
	}, onProgress)

	graph := models.NewDependencyGraph()
	nodeMap := make(map[string]bool)

	// Collect all nodes
	for _, fd := range fileData {
		for _, node := range fd.nodes {
			if !nodeMap[node.ID] {
				graph.AddNode(node)
				nodeMap[node.ID] = true
			}
		}
	}

	// Build edges using collected data
	var mu sync.Mutex
	for _, fd := range fileData {
		switch a.scope {
		case ScopeFile:
			for _, imp := range fd.imports {
				for targetPath := range nodeMap {
					if matchesImport(targetPath, imp) {
						mu.Lock()
						graph.AddEdge(models.GraphEdge{
							From: fd.nodes[0].ID,
							To:   targetPath,
							Type: models.EdgeImport,
						})
						mu.Unlock()
					}
				}
			}

		case ScopeFunction:
			for sourceID, calls := range fd.calls {
				for _, call := range calls {
					for targetID := range nodeMap {
						if matchesCall(targetID, call) {
							mu.Lock()
							graph.AddEdge(models.GraphEdge{
								From: sourceID,
								To:   targetID,
								Type: models.EdgeCall,
							})
							mu.Unlock()
						}
					}
				}
			}
		}
	}

	return graph, nil
}

// analyzeFileGraph extracts graph data from a single file.
func (a *GraphAnalyzer) analyzeFileGraph(psr *parser.Parser, path string) (fileGraphData, error) {
	result, err := psr.ParseFile(path)
	if err != nil {
		return fileGraphData{}, err
	}

	fd := fileGraphData{
		calls: make(map[string][]string),
	}

	switch a.scope {
	case ScopeFile:
		fd.nodes = append(fd.nodes, models.GraphNode{
			ID:   path,
			Name: path,
			Type: models.NodeFile,
			File: path,
		})
		fd.imports = extractImports(result)

	case ScopeFunction:
		functions := parser.GetFunctions(result)
		for _, fn := range functions {
			nodeID := path + ":" + fn.Name
			fd.nodes = append(fd.nodes, models.GraphNode{
				ID:   nodeID,
				Name: fn.Name,
				Type: models.NodeFunction,
				File: path,
				Line: fn.StartLine,
			})
			fd.calls[nodeID] = extractCalls(fn.Body, result.Source)
		}

	case ScopeModule:
		moduleName := extractModuleName(result)
		if moduleName != "" {
			fd.nodes = append(fd.nodes, models.GraphNode{
				ID:   moduleName,
				Name: moduleName,
				Type: models.NodeModule,
				File: path,
			})
		}
	}

	return fd, nil
}

// extractModuleName extracts the module/package name from source.
func extractModuleName(result *parser.ParseResult) string {
	root := result.Tree.RootNode()

	switch result.Language {
	case parser.LangGo:
		// Look for package declaration
		nodes := parser.FindNodesByType(root, result.Source, "package_clause")
		if len(nodes) > 0 {
			if nameNode := nodes[0].ChildByFieldName("name"); nameNode != nil {
				return parser.GetNodeText(nameNode, result.Source)
			}
		}

	case parser.LangRust:
		// Rust uses crate/mod structure
		nodes := parser.FindNodesByType(root, result.Source, "mod_item")
		if len(nodes) > 0 {
			if nameNode := nodes[0].ChildByFieldName("name"); nameNode != nil {
				return parser.GetNodeText(nameNode, result.Source)
			}
		}

	case parser.LangPython:
		// Python modules are typically files
		return result.Path

	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		// JS/TS modules are files
		return result.Path
	}

	return ""
}

// extractImports extracts import statements from source.
func extractImports(result *parser.ParseResult) []string {
	var imports []string
	root := result.Tree.RootNode()

	importTypes := getImportNodeTypes(result.Language)

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, it := range importTypes {
			if node.Type() == it {
				imp := extractImportPath(node, source, result.Language)
				if imp != "" {
					imports = append(imports, imp)
				}
			}
		}
		return true
	})

	return imports
}

// getImportNodeTypes returns AST node types for imports.
func getImportNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangGo:
		return []string{"import_spec"}
	case parser.LangRust:
		return []string{"use_declaration"}
	case parser.LangPython:
		return []string{"import_statement", "import_from_statement"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"import_statement", "import_declaration"}
	case parser.LangJava:
		return []string{"import_declaration"}
	case parser.LangRuby:
		return []string{"call"} // require, require_relative, load are method calls
	default:
		return []string{"import_statement", "import_declaration"}
	}
}

// extractImportPath extracts the import path from an import node.
func extractImportPath(node *sitter.Node, source []byte, lang parser.Language) string {
	switch lang {
	case parser.LangGo:
		if pathNode := node.ChildByFieldName("path"); pathNode != nil {
			path := parser.GetNodeText(pathNode, source)
			if len(path) >= 2 {
				return path[1 : len(path)-1]
			}
		}

	case parser.LangPython:
		if modNode := node.ChildByFieldName("module_name"); modNode != nil {
			return parser.GetNodeText(modNode, source)
		}
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			return parser.GetNodeText(nameNode, source)
		}

	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		if sourceNode := node.ChildByFieldName("source"); sourceNode != nil {
			path := parser.GetNodeText(sourceNode, source)
			if len(path) >= 2 {
				return path[1 : len(path)-1]
			}
		}

	case parser.LangRuby:
		// Ruby uses require/require_relative/load as method calls
		if methodNode := node.ChildByFieldName("method"); methodNode != nil {
			method := parser.GetNodeText(methodNode, source)
			if method == "require" || method == "require_relative" || method == "load" {
				if argsNode := node.ChildByFieldName("arguments"); argsNode != nil {
					// Find the string argument
					for i := range int(argsNode.ChildCount()) {
						child := argsNode.Child(i)
						if child.Type() == "string" {
							path := parser.GetNodeText(child, source)
							// Remove quotes
							if len(path) >= 2 {
								return path[1 : len(path)-1]
							}
						}
					}
				}
			}
		}

	default:
		// Generic: look for string literals
		for i := range int(node.ChildCount()) {
			child := node.Child(i)
			if child.Type() == "string" || child.Type() == "string_literal" {
				path := parser.GetNodeText(child, source)
				if len(path) >= 2 {
					return path[1 : len(path)-1]
				}
			}
		}
	}

	return ""
}

// extractCalls extracts function call names from a function body.
func extractCalls(body *sitter.Node, source []byte) []string {
	if body == nil {
		return nil
	}

	var calls []string
	callTypes := []string{"call_expression", "function_call", "method_call"}

	parser.Walk(body, source, func(node *sitter.Node, src []byte) bool {
		for _, ct := range callTypes {
			if node.Type() == ct {
				// Extract function name
				if fnNode := node.ChildByFieldName("function"); fnNode != nil {
					calls = append(calls, parser.GetNodeText(fnNode, src))
				} else if fnNode := node.ChildByFieldName("name"); fnNode != nil {
					calls = append(calls, parser.GetNodeText(fnNode, src))
				}
			}
		}
		return true
	})

	return calls
}

// matchesImport checks if a file path matches an import path.
func matchesImport(filePath, importPath string) bool {
	// Simple substring matching for now
	return filePath != importPath && // Not self-reference
		(contains(filePath, importPath) || contains(importPath, filePath))
}

// matchesCall checks if a node ID matches a function call.
func matchesCall(nodeID, callName string) bool {
	// Node IDs are "path:funcName", calls are just "funcName"
	return contains(nodeID, ":"+callName)
}

// contains checks if s contains substr.
func contains(s, substr string) bool {
	for i := 0; i+len(substr) <= len(s); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// CalculateMetrics computes graph metrics like PageRank.
func (a *GraphAnalyzer) CalculateMetrics(graph *models.DependencyGraph) *models.GraphMetrics {
	metrics := &models.GraphMetrics{
		NodeMetrics: make([]models.NodeMetric, 0),
		Summary: models.GraphSummary{
			TotalNodes: len(graph.Nodes),
			TotalEdges: len(graph.Edges),
		},
	}

	// Build adjacency lists
	inDegree := make(map[string]int)
	outDegree := make(map[string]int)
	outNeighbors := make(map[string][]string)

	for _, node := range graph.Nodes {
		inDegree[node.ID] = 0
		outDegree[node.ID] = 0
		outNeighbors[node.ID] = []string{}
	}

	for _, edge := range graph.Edges {
		inDegree[edge.To]++
		outDegree[edge.From]++
		outNeighbors[edge.From] = append(outNeighbors[edge.From], edge.To)
	}

	// Calculate PageRank
	pageRank := calculatePageRank(graph.Nodes, outNeighbors, 20, 0.85)

	// Calculate betweenness centrality (simplified)
	betweenness := calculateBetweenness(graph.Nodes, outNeighbors)

	// Build node metrics
	for _, node := range graph.Nodes {
		nm := models.NodeMetric{
			NodeID:                node.ID,
			Name:                  node.Name,
			PageRank:              pageRank[node.ID],
			BetweennessCentrality: betweenness[node.ID],
			InDegree:              inDegree[node.ID],
			OutDegree:             outDegree[node.ID],
		}
		metrics.NodeMetrics = append(metrics.NodeMetrics, nm)
	}

	// Calculate summary statistics
	if len(graph.Nodes) > 0 {
		totalDegree := 0
		for _, node := range graph.Nodes {
			totalDegree += inDegree[node.ID] + outDegree[node.ID]
		}
		metrics.Summary.AvgDegree = float64(totalDegree) / float64(len(graph.Nodes))

		// Density = E / (V * (V-1))
		if len(graph.Nodes) > 1 {
			maxEdges := len(graph.Nodes) * (len(graph.Nodes) - 1)
			metrics.Summary.Density = float64(len(graph.Edges)) / float64(maxEdges)
		}
	}

	return metrics
}

// calculatePageRank computes PageRank scores for nodes.
func calculatePageRank(nodes []models.GraphNode, outNeighbors map[string][]string, iterations int, damping float64) map[string]float64 {
	n := float64(len(nodes))
	if n == 0 {
		return make(map[string]float64)
	}

	// Initialize scores
	scores := make(map[string]float64)
	for _, node := range nodes {
		scores[node.ID] = 1.0 / n
	}

	// Iterate
	for range iterations {
		newScores := make(map[string]float64)
		for _, node := range nodes {
			newScores[node.ID] = (1 - damping) / n
		}

		for _, node := range nodes {
			neighbors := outNeighbors[node.ID]
			if len(neighbors) > 0 {
				share := scores[node.ID] / float64(len(neighbors))
				for _, neighbor := range neighbors {
					newScores[neighbor] += damping * share
				}
			} else {
				// Dangling node: distribute evenly
				share := scores[node.ID] / n
				for _, other := range nodes {
					newScores[other.ID] += damping * share
				}
			}
		}

		scores = newScores
	}

	return scores
}

// calculateBetweenness computes simplified betweenness centrality.
func calculateBetweenness(nodes []models.GraphNode, outNeighbors map[string][]string) map[string]float64 {
	betweenness := make(map[string]float64)
	for _, node := range nodes {
		betweenness[node.ID] = 0
	}

	// Simplified: count paths through each node
	for _, source := range nodes {
		visited := make(map[string]bool)
		queue := []string{source.ID}
		visited[source.ID] = true

		for len(queue) > 0 {
			current := queue[0]
			queue = queue[1:]

			for _, neighbor := range outNeighbors[current] {
				if !visited[neighbor] {
					visited[neighbor] = true
					queue = append(queue, neighbor)
					if current != source.ID {
						betweenness[current]++
					}
				}
			}
		}
	}

	// Normalize
	n := float64(len(nodes))
	if n > 2 {
		norm := (n - 1) * (n - 2)
		for id := range betweenness {
			betweenness[id] /= norm
		}
	}

	return betweenness
}

// Close releases analyzer resources.
func (a *GraphAnalyzer) Close() {
	a.parser.Close()
}

// DetectCycles uses Tarjan's algorithm to find strongly connected components (cycles).
func (a *GraphAnalyzer) DetectCycles(graph *models.DependencyGraph) [][]string {
	return tarjanSCC(graph)
}

// tarjanSCC implements Tarjan's strongly connected components algorithm.
func tarjanSCC(graph *models.DependencyGraph) [][]string {
	n := len(graph.Nodes)
	if n == 0 {
		return nil
	}

	// Build node index mapping and adjacency list
	nodeIndex := make(map[string]int)
	for i, node := range graph.Nodes {
		nodeIndex[node.ID] = i
	}

	adj := make([][]int, n)
	for i := range adj {
		adj[i] = []int{}
	}
	for _, edge := range graph.Edges {
		fromIdx, fromOK := nodeIndex[edge.From]
		toIdx, toOK := nodeIndex[edge.To]
		if fromOK && toOK {
			adj[fromIdx] = append(adj[fromIdx], toIdx)
		}
	}

	// Tarjan's algorithm state
	index := 0
	indices := make([]int, n)
	lowLinks := make([]int, n)
	onStack := make([]bool, n)
	stack := make([]int, 0, n)
	for i := range n {
		indices[i] = -1
	}

	var sccs [][]string

	var strongConnect func(v int)
	strongConnect = func(v int) {
		indices[v] = index
		lowLinks[v] = index
		index++
		stack = append(stack, v)
		onStack[v] = true

		for _, w := range adj[v] {
			if indices[w] == -1 {
				strongConnect(w)
				if lowLinks[w] < lowLinks[v] {
					lowLinks[v] = lowLinks[w]
				}
			} else if onStack[w] {
				if indices[w] < lowLinks[v] {
					lowLinks[v] = indices[w]
				}
			}
		}

		// Root of an SCC
		if lowLinks[v] == indices[v] {
			var scc []string
			for {
				w := stack[len(stack)-1]
				stack = stack[:len(stack)-1]
				onStack[w] = false
				scc = append(scc, graph.Nodes[w].ID)
				if w == v {
					break
				}
			}
			// Only include SCCs with more than one node (actual cycles)
			if len(scc) > 1 {
				sccs = append(sccs, scc)
			}
		}
	}

	for i := range n {
		if indices[i] == -1 {
			strongConnect(i)
		}
	}

	return sccs
}

// rankedNode pairs a graph node with its PageRank score for sorting.
type rankedNode struct {
	node models.GraphNode
	rank float64
}

// PruneGraph reduces graph size while preserving important nodes using PageRank.
func (a *GraphAnalyzer) PruneGraph(graph *models.DependencyGraph, maxNodes, maxEdges int) *models.DependencyGraph {
	if len(graph.Nodes) <= maxNodes && len(graph.Edges) <= maxEdges {
		return graph
	}

	// Build adjacency list for PageRank
	outNeighbors := make(map[string][]string)
	for _, node := range graph.Nodes {
		outNeighbors[node.ID] = []string{}
	}
	for _, edge := range graph.Edges {
		outNeighbors[edge.From] = append(outNeighbors[edge.From], edge.To)
	}

	// Calculate PageRank to determine node importance
	pageRank := calculatePageRank(graph.Nodes, outNeighbors, 20, 0.85)

	// Sort nodes by PageRank (highest first)
	ranked := make([]rankedNode, len(graph.Nodes))
	for i, node := range graph.Nodes {
		ranked[i] = rankedNode{node: node, rank: pageRank[node.ID]}
	}
	sortRankedNodes(ranked)

	// Select top nodes
	pruned := models.NewDependencyGraph()
	nodeSet := make(map[string]bool)

	limit := maxNodes
	if limit > len(ranked) {
		limit = len(ranked)
	}
	for i := 0; i < limit; i++ {
		pruned.AddNode(ranked[i].node)
		nodeSet[ranked[i].node.ID] = true
	}

	// Add edges between selected nodes
	edgeCount := 0
	for _, edge := range graph.Edges {
		if edgeCount >= maxEdges {
			break
		}
		if nodeSet[edge.From] && nodeSet[edge.To] {
			pruned.AddEdge(edge)
			edgeCount++
		}
	}

	return pruned
}

// sortRankedNodes sorts nodes by PageRank in descending order using insertion sort.
func sortRankedNodes(nodes []rankedNode) {
	for i := 1; i < len(nodes); i++ {
		key := nodes[i]
		j := i - 1
		for j >= 0 && nodes[j].rank < key.rank {
			nodes[j+1] = nodes[j]
			j--
		}
		nodes[j+1] = key
	}
}
