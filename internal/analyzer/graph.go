package analyzer

import (
	"strings"
	"sync"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
	"gonum.org/v1/gonum/graph/community"
	"gonum.org/v1/gonum/graph/network"
	"gonum.org/v1/gonum/graph/path"
	"gonum.org/v1/gonum/graph/simple"
	"gonum.org/v1/gonum/graph/topo"
)

// GraphAnalyzer builds dependency graphs from source code.
type GraphAnalyzer struct {
	parser      *parser.Parser
	scope       GraphScope
	maxFileSize int64
}

// GraphScope determines the granularity of graph nodes.
type GraphScope string

const (
	ScopeFile     GraphScope = "file"
	ScopeFunction GraphScope = "function"
	ScopeModule   GraphScope = "module"
	ScopePackage  GraphScope = "package"
)

// GraphOption is a functional option for configuring GraphAnalyzer.
type GraphOption func(*GraphAnalyzer)

// WithGraphScope sets the graph node granularity.
func WithGraphScope(scope GraphScope) GraphOption {
	return func(a *GraphAnalyzer) {
		a.scope = scope
	}
}

// WithGraphMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithGraphMaxFileSize(maxSize int64) GraphOption {
	return func(a *GraphAnalyzer) {
		a.maxFileSize = maxSize
	}
}

// NewGraphAnalyzer creates a new graph analyzer.
func NewGraphAnalyzer(opts ...GraphOption) *GraphAnalyzer {
	a := &GraphAnalyzer{
		parser:      parser.New(),
		scope:       ScopeFunction,
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
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
func (a *GraphAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress fileproc.ProgressFunc) (*models.DependencyGraph, error) {
	// Parse all files in parallel, extracting nodes and edge data
	fileData := fileproc.MapFilesWithSizeLimit(files, a.maxFileSize, func(psr *parser.Parser, path string) (fileGraphData, error) {
		return a.analyzeFileGraph(psr, path)
	}, onProgress, nil)

	graph := models.NewDependencyGraph()
	nodeMap := make(map[string]bool)

	// Build index for O(1) function name lookup: funcName -> []nodeID
	funcNameIndex := make(map[string][]string)

	// Collect all nodes and build index
	for _, fd := range fileData {
		for _, node := range fd.nodes {
			if !nodeMap[node.ID] {
				graph.AddNode(node)
				nodeMap[node.ID] = true

				// For function scope, index by function name for O(1) lookup
				if a.scope == ScopeFunction {
					if idx := strings.LastIndex(node.ID, ":"); idx >= 0 {
						funcName := node.ID[idx+1:]
						funcNameIndex[funcName] = append(funcNameIndex[funcName], node.ID)
					}
				}
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
					// Use index for O(1) lookup instead of O(n) iteration
					funcName := call
					if idx := strings.LastIndex(call, "."); idx >= 0 {
						funcName = call[idx+1:]
					}

					if targetIDs, ok := funcNameIndex[funcName]; ok {
						mu.Lock()
						for _, targetID := range targetIDs {
							if targetID != sourceID {
								graph.AddEdge(models.GraphEdge{
									From: sourceID,
									To:   targetID,
									Type: models.EdgeCall,
								})
							}
						}
						mu.Unlock()
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
	return filePath != importPath &&
		(strings.Contains(filePath, importPath) || strings.Contains(importPath, filePath))
}

// gonumGraph holds the gonum representation and mappings.
type gonumGraph struct {
	directed   *simple.DirectedGraph
	undirected *simple.UndirectedGraph
	nodeIDToID map[string]int64 // our node ID -> gonum int64 ID
	idToNodeID map[int64]string // gonum int64 ID -> our node ID
}

// toGonumGraph converts our DependencyGraph to gonum graph types.
func toGonumGraph(graph *models.DependencyGraph) *gonumGraph {
	g := &gonumGraph{
		directed:   simple.NewDirectedGraph(),
		undirected: simple.NewUndirectedGraph(),
		nodeIDToID: make(map[string]int64),
		idToNodeID: make(map[int64]string),
	}

	// Create nodes with sequential IDs
	for i, node := range graph.Nodes {
		id := int64(i)
		g.nodeIDToID[node.ID] = id
		g.idToNodeID[id] = node.ID
		g.directed.AddNode(simple.Node(id))
		g.undirected.AddNode(simple.Node(id))
	}

	// Add edges (skip self-loops as gonum simple graphs don't support them)
	for _, edge := range graph.Edges {
		fromID, fromOK := g.nodeIDToID[edge.From]
		toID, toOK := g.nodeIDToID[edge.To]
		if fromOK && toOK && fromID != toID {
			g.directed.SetEdge(simple.Edge{F: simple.Node(fromID), T: simple.Node(toID)})
			// For undirected, only add if not already present (avoid duplicate)
			if !g.undirected.HasEdgeBetween(fromID, toID) {
				g.undirected.SetEdge(simple.Edge{F: simple.Node(fromID), T: simple.Node(toID)})
			}
		}
	}

	return g
}

// sparsePageRank computes PageRank using sparse power iteration.
// This is O(E * iterations) instead of gonum's O(V^2 * iterations).
func sparsePageRank(nodes []models.GraphNode, edges []models.GraphEdge, damping float64, tolerance float64) map[string]float64 {
	n := len(nodes)
	if n == 0 {
		return make(map[string]float64)
	}

	// Build node index
	nodeIndex := make(map[string]int, n)
	for i, node := range nodes {
		nodeIndex[node.ID] = i
	}

	// Build adjacency lists: outNeighbors[i] = list of nodes that i points to
	outNeighbors := make([][]int, n)
	outDegree := make([]int, n)
	for i := range outNeighbors {
		outNeighbors[i] = make([]int, 0)
	}
	for _, edge := range edges {
		fromIdx := nodeIndex[edge.From]
		toIdx := nodeIndex[edge.To]
		outNeighbors[fromIdx] = append(outNeighbors[fromIdx], toIdx)
		outDegree[fromIdx]++
	}

	// Initialize PageRank
	rank := make([]float64, n)
	newRank := make([]float64, n)
	initial := 1.0 / float64(n)
	for i := range rank {
		rank[i] = initial
	}

	dampingFactor := damping
	teleport := (1.0 - dampingFactor) / float64(n)
	maxIterations := 100

	for iter := 0; iter < maxIterations; iter++ {
		// Reset new ranks
		for i := range newRank {
			newRank[i] = teleport
		}

		// Distribute rank along edges
		for i := 0; i < n; i++ {
			if outDegree[i] > 0 {
				contrib := dampingFactor * rank[i] / float64(outDegree[i])
				for _, j := range outNeighbors[i] {
					newRank[j] += contrib
				}
			} else {
				// Dangling node: distribute to all nodes
				contrib := dampingFactor * rank[i] / float64(n)
				for j := range newRank {
					newRank[j] += contrib
				}
			}
		}

		// Check convergence
		diff := 0.0
		for i := range rank {
			d := newRank[i] - rank[i]
			if d < 0 {
				d = -d
			}
			diff += d
		}

		// Swap
		rank, newRank = newRank, rank

		if diff < tolerance {
			break
		}
	}

	// Convert back to map
	result := make(map[string]float64, n)
	for i, node := range nodes {
		result[node.ID] = rank[i]
	}
	return result
}

// CalculatePageRankOnly computes only PageRank and degree metrics for fast ranking.
// Use this when you only need ranking (e.g., repo map) instead of full metrics.
func (a *GraphAnalyzer) CalculatePageRankOnly(graph *models.DependencyGraph) *models.GraphMetrics {
	metrics := &models.GraphMetrics{
		NodeMetrics: make([]models.NodeMetric, 0, len(graph.Nodes)),
		Summary: models.GraphSummary{
			TotalNodes: len(graph.Nodes),
			TotalEdges: len(graph.Edges),
		},
	}

	if len(graph.Nodes) == 0 {
		return metrics
	}

	// Build degree maps
	inDegree := make(map[string]int, len(graph.Nodes))
	outDegree := make(map[string]int, len(graph.Nodes))
	for _, node := range graph.Nodes {
		inDegree[node.ID] = 0
		outDegree[node.ID] = 0
	}
	for _, edge := range graph.Edges {
		inDegree[edge.To]++
		outDegree[edge.From]++
	}

	// Use sparse PageRank for O(E * iterations) instead of O(V^2 * iterations)
	pageRank := sparsePageRank(graph.Nodes, graph.Edges, 0.85, 1e-6)

	// Build node metrics (only PageRank and degrees)
	for _, node := range graph.Nodes {
		nm := models.NodeMetric{
			NodeID:    node.ID,
			Name:      node.Name,
			PageRank:  pageRank[node.ID],
			InDegree:  inDegree[node.ID],
			OutDegree: outDegree[node.ID],
		}
		metrics.NodeMetrics = append(metrics.NodeMetrics, nm)
	}

	// Calculate summary
	totalDegree := 0
	for _, node := range graph.Nodes {
		totalDegree += inDegree[node.ID] + outDegree[node.ID]
	}
	metrics.Summary.AvgDegree = float64(totalDegree) / float64(len(graph.Nodes))

	if len(graph.Nodes) > 1 {
		maxEdges := len(graph.Nodes) * (len(graph.Nodes) - 1)
		metrics.Summary.Density = float64(len(graph.Edges)) / float64(maxEdges)
	}

	return metrics
}

// CalculateMetrics computes comprehensive graph metrics.
func (a *GraphAnalyzer) CalculateMetrics(graph *models.DependencyGraph) *models.GraphMetrics {
	metrics := &models.GraphMetrics{
		NodeMetrics: make([]models.NodeMetric, 0),
		Summary: models.GraphSummary{
			TotalNodes: len(graph.Nodes),
			TotalEdges: len(graph.Edges),
		},
	}

	if len(graph.Nodes) == 0 {
		return metrics
	}

	// Convert to gonum graph
	gGraph := toGonumGraph(graph)

	// Build adjacency lists (still needed for some hand-rolled algorithms)
	inDegree := make(map[string]int)
	outDegree := make(map[string]int)
	outNeighbors := make(map[string][]string)
	inNeighbors := make(map[string][]string)

	for _, node := range graph.Nodes {
		inDegree[node.ID] = 0
		outDegree[node.ID] = 0
		outNeighbors[node.ID] = []string{}
		inNeighbors[node.ID] = []string{}
	}

	for _, edge := range graph.Edges {
		inDegree[edge.To]++
		outDegree[edge.From]++
		outNeighbors[edge.From] = append(outNeighbors[edge.From], edge.To)
		inNeighbors[edge.To] = append(inNeighbors[edge.To], edge.From)
	}

	// Compute centrality metrics in parallel for better performance
	var pageRankMap, betweennessMap, closenessMap, harmonicMap map[int64]float64
	var allShortest path.AllShortest
	var wg sync.WaitGroup

	// Phase 1: Compute PageRank, Betweenness, and AllShortest in parallel
	wg.Add(3)
	go func() {
		defer wg.Done()
		pageRankMap = network.PageRank(gGraph.directed, 0.85, 1e-6)
	}()
	go func() {
		defer wg.Done()
		betweennessMap = network.Betweenness(gGraph.directed)
	}()
	go func() {
		defer wg.Done()
		allShortest = path.DijkstraAllPaths(gGraph.directed)
	}()
	wg.Wait()

	// Phase 2: Compute Closeness and Harmonic in parallel (both need AllShortest)
	wg.Add(2)
	go func() {
		defer wg.Done()
		closenessMap = network.Closeness(gGraph.directed, allShortest)
	}()
	go func() {
		defer wg.Done()
		harmonicMap = network.Harmonic(gGraph.directed, allShortest)
	}()
	wg.Wait()

	// Convert gonum maps (int64 keys) to our string-keyed maps
	pageRank := make(map[string]float64)
	betweenness := make(map[string]float64)
	closeness := make(map[string]float64)
	harmonic := make(map[string]float64)

	for id, nodeID := range gGraph.idToNodeID {
		pageRank[nodeID] = pageRankMap[id]
		betweenness[nodeID] = betweennessMap[id]
		closeness[nodeID] = closenessMap[id]
		harmonic[nodeID] = harmonicMap[id]
	}

	// Hand-rolled algorithms (gonum doesn't provide these)
	eigenvector := calculateEigenvector(graph.Nodes, inNeighbors, 100, 1e-6)
	clustering := calculateClusteringCoefficients(graph.Nodes, outNeighbors)

	// Community detection using gonum's Louvain implementation
	communities, modularity := gonumCommunityDetection(gGraph)

	// Build node metrics
	for _, node := range graph.Nodes {
		nm := models.NodeMetric{
			NodeID:                node.ID,
			Name:                  node.Name,
			PageRank:              pageRank[node.ID],
			BetweennessCentrality: betweenness[node.ID],
			ClosenessCentrality:   closeness[node.ID],
			EigenvectorCentrality: eigenvector[node.ID],
			HarmonicCentrality:    harmonic[node.ID],
			InDegree:              inDegree[node.ID],
			OutDegree:             outDegree[node.ID],
			ClusteringCoef:        clustering[node.ID],
			CommunityID:           communities[node.ID],
		}
		metrics.NodeMetrics = append(metrics.NodeMetrics, nm)
	}

	// Calculate summary statistics
	totalDegree := 0
	for _, node := range graph.Nodes {
		totalDegree += inDegree[node.ID] + outDegree[node.ID]
	}
	metrics.Summary.AvgDegree = float64(totalDegree) / float64(len(graph.Nodes))

	// Density = E / (V * (V-1)) for directed graph
	if len(graph.Nodes) > 1 {
		maxEdges := len(graph.Nodes) * (len(graph.Nodes) - 1)
		metrics.Summary.Density = float64(len(graph.Edges)) / float64(maxEdges)
	}

	// Use gonum for connected components (undirected)
	gonumComponents := topo.ConnectedComponents(gGraph.undirected)
	metrics.Summary.Components = len(gonumComponents)
	largestComponent := 0
	for _, comp := range gonumComponents {
		if len(comp) > largestComponent {
			largestComponent = len(comp)
		}
	}
	metrics.Summary.LargestComponent = largestComponent

	// Use gonum for strongly connected components
	gonumSCCs := topo.TarjanSCC(gGraph.directed)
	// Filter to only SCCs with more than one node (actual cycles)
	var sccs [][]string
	for _, scc := range gonumSCCs {
		if len(scc) > 1 {
			var nodeIDs []string
			for _, node := range scc {
				nodeIDs = append(nodeIDs, gGraph.idToNodeID[node.ID()])
			}
			sccs = append(sccs, nodeIDs)
		}
	}
	metrics.Summary.StronglyConnectedComponents = len(sccs)
	metrics.Summary.CycleCount = len(sccs)
	metrics.Summary.IsCyclic = len(sccs) > 0

	// Collect cycle nodes
	cycleNodeSet := make(map[string]bool)
	for _, scc := range sccs {
		for _, nodeID := range scc {
			cycleNodeSet[nodeID] = true
		}
	}
	for nodeID := range cycleNodeSet {
		metrics.Summary.CycleNodes = append(metrics.Summary.CycleNodes, nodeID)
	}

	// Diameter and radius (hand-rolled)
	diameter, radius := calculateDiameterAndRadius(graph.Nodes, outNeighbors)
	metrics.Summary.Diameter = diameter
	metrics.Summary.Radius = radius

	// Global clustering coefficient (transitivity) - hand-rolled
	metrics.Summary.ClusteringCoefficient = calculateGlobalClustering(graph.Nodes, outNeighbors)
	metrics.Summary.Transitivity = metrics.Summary.ClusteringCoefficient

	// Assortativity - hand-rolled
	metrics.Summary.Assortativity = calculateAssortativity(graph)

	// Reciprocity - hand-rolled
	metrics.Summary.Reciprocity = calculateReciprocity(graph)

	// Community metrics
	communitySet := make(map[int]bool)
	for _, c := range communities {
		communitySet[c] = true
	}
	metrics.Summary.CommunityCount = len(communitySet)
	metrics.Summary.Modularity = modularity

	return metrics
}

// calculateEigenvector computes eigenvector centrality using power iteration.
func calculateEigenvector(nodes []models.GraphNode, inNeighbors map[string][]string, iterations int, tolerance float64) map[string]float64 {
	n := len(nodes)
	if n == 0 {
		return make(map[string]float64)
	}

	// Initialize scores uniformly
	scores := make(map[string]float64)
	for _, node := range nodes {
		scores[node.ID] = 1.0 / float64(n)
	}

	// Power iteration
	for iter := 0; iter < iterations; iter++ {
		newScores := make(map[string]float64)

		// Each node's score is the sum of its neighbors' scores
		for _, node := range nodes {
			sum := 0.0
			for _, neighbor := range inNeighbors[node.ID] {
				sum += scores[neighbor]
			}
			newScores[node.ID] = sum
		}

		// Normalize
		norm := 0.0
		for _, score := range newScores {
			norm += score * score
		}
		norm = sqrt(norm)
		if norm > 0 {
			for id := range newScores {
				newScores[id] /= norm
			}
		}

		// Check convergence
		maxDiff := 0.0
		for id, score := range newScores {
			diff := score - scores[id]
			if diff < 0 {
				diff = -diff
			}
			if diff > maxDiff {
				maxDiff = diff
			}
		}

		scores = newScores
		if maxDiff < tolerance {
			break
		}
	}

	return scores
}

// sqrt computes square root using Newton's method.
func sqrt(x float64) float64 {
	if x <= 0 {
		return 0
	}
	z := x
	for i := 0; i < 20; i++ {
		z = (z + x/z) / 2
	}
	return z
}

// calculateClusteringCoefficients computes local clustering coefficient for each node.
func calculateClusteringCoefficients(nodes []models.GraphNode, outNeighbors map[string][]string) map[string]float64 {
	clustering := make(map[string]float64)

	// Build bidirectional neighbor sets for undirected interpretation
	neighbors := make(map[string]map[string]bool)
	for _, node := range nodes {
		neighbors[node.ID] = make(map[string]bool)
	}
	for id, out := range outNeighbors {
		for _, neighbor := range out {
			neighbors[id][neighbor] = true
			neighbors[neighbor][id] = true
		}
	}

	for _, node := range nodes {
		neighborSet := neighbors[node.ID]
		k := len(neighborSet)
		if k < 2 {
			clustering[node.ID] = 0
			continue
		}

		// Count edges between neighbors
		triangles := 0
		neighborList := make([]string, 0, k)
		for n := range neighborSet {
			neighborList = append(neighborList, n)
		}

		for i := 0; i < len(neighborList); i++ {
			for j := i + 1; j < len(neighborList); j++ {
				if neighbors[neighborList[i]][neighborList[j]] {
					triangles++
				}
			}
		}

		// Local clustering coefficient
		maxTriangles := k * (k - 1) / 2
		if maxTriangles > 0 {
			clustering[node.ID] = float64(triangles) / float64(maxTriangles)
		}
	}

	return clustering
}

// calculateGlobalClustering computes global clustering coefficient (transitivity).
func calculateGlobalClustering(nodes []models.GraphNode, outNeighbors map[string][]string) float64 {
	// Build bidirectional neighbor sets
	neighbors := make(map[string]map[string]bool)
	for _, node := range nodes {
		neighbors[node.ID] = make(map[string]bool)
	}
	for id, out := range outNeighbors {
		for _, neighbor := range out {
			neighbors[id][neighbor] = true
			neighbors[neighbor][id] = true
		}
	}

	triangles := 0
	triplets := 0

	for _, node := range nodes {
		neighborList := make([]string, 0)
		for n := range neighbors[node.ID] {
			neighborList = append(neighborList, n)
		}
		k := len(neighborList)

		// Count triplets centered on this node
		triplets += k * (k - 1) / 2

		// Count triangles
		for i := 0; i < k; i++ {
			for j := i + 1; j < k; j++ {
				if neighbors[neighborList[i]][neighborList[j]] {
					triangles++
				}
			}
		}
	}

	if triplets > 0 {
		return float64(triangles) / float64(triplets)
	}
	return 0
}

// calculateAssortativity computes degree assortativity coefficient.
func calculateAssortativity(graph *models.DependencyGraph) float64 {
	if len(graph.Edges) == 0 {
		return 0
	}

	// Calculate degrees
	degree := make(map[string]int)
	for _, node := range graph.Nodes {
		degree[node.ID] = 0
	}
	for _, edge := range graph.Edges {
		degree[edge.From]++
		degree[edge.To]++
	}

	// Compute assortativity using Pearson correlation of degrees at edge endpoints
	var sumXY, sumX, sumY, sumX2, sumY2 float64
	m := float64(len(graph.Edges))

	for _, edge := range graph.Edges {
		x := float64(degree[edge.From])
		y := float64(degree[edge.To])
		sumXY += x * y
		sumX += x
		sumY += y
		sumX2 += x * x
		sumY2 += y * y
	}

	// Pearson correlation coefficient
	num := sumXY - (sumX*sumY)/m
	denom1 := sumX2 - (sumX*sumX)/m
	denom2 := sumY2 - (sumY*sumY)/m

	if denom1 > 0 && denom2 > 0 {
		return num / sqrt(denom1*denom2)
	}
	return 0
}

// calculateReciprocity computes the fraction of edges that have a reverse edge.
func calculateReciprocity(graph *models.DependencyGraph) float64 {
	if len(graph.Edges) == 0 {
		return 0
	}

	// Build edge set
	edgeSet := make(map[string]bool)
	for _, edge := range graph.Edges {
		edgeSet[edge.From+":"+edge.To] = true
	}

	// Count reciprocal edges
	reciprocal := 0
	for _, edge := range graph.Edges {
		reverseKey := edge.To + ":" + edge.From
		if edgeSet[reverseKey] {
			reciprocal++
		}
	}

	return float64(reciprocal) / float64(len(graph.Edges))
}

// calculateDiameterAndRadius computes diameter and radius using BFS.
// For large graphs (>1000 nodes), uses sampling for performance.
func calculateDiameterAndRadius(nodes []models.GraphNode, outNeighbors map[string][]string) (int, int) {
	if len(nodes) == 0 {
		return 0, 0
	}

	// Build undirected adjacency for diameter calculation
	neighbors := make(map[string][]string, len(nodes))
	for _, node := range nodes {
		neighbors[node.ID] = nil
	}
	for id, out := range outNeighbors {
		for _, neighbor := range out {
			neighbors[id] = append(neighbors[id], neighbor)
			neighbors[neighbor] = append(neighbors[neighbor], id)
		}
	}

	// For large graphs, sample nodes instead of checking all
	sampleSize := len(nodes)
	if sampleSize > 100 {
		sampleSize = 100 // Sample 100 nodes max for O(100*n) instead of O(n^2)
	}

	// Pre-allocate distance map once and reuse
	dist := make(map[string]int, len(nodes))
	queue := make([]string, 0, len(nodes))

	diameter := 0
	minEccentricity := -1

	// Sample evenly across the node list
	step := len(nodes) / sampleSize
	if step < 1 {
		step = 1
	}

	for i := 0; i < len(nodes) && i/step < sampleSize; i += step {
		source := nodes[i]

		// Reset distance map
		for k := range dist {
			delete(dist, k)
		}
		for _, n := range nodes {
			dist[n.ID] = -1
		}
		dist[source.ID] = 0

		// Reset queue
		queue = queue[:0]
		queue = append(queue, source.ID)
		maxDist := 0

		for len(queue) > 0 {
			current := queue[0]
			queue = queue[1:]

			for _, neighbor := range neighbors[current] {
				if dist[neighbor] < 0 {
					dist[neighbor] = dist[current] + 1
					if dist[neighbor] > maxDist {
						maxDist = dist[neighbor]
					}
					queue = append(queue, neighbor)
				}
			}
		}

		// Check if graph is connected from this node
		connected := true
		for _, n := range nodes {
			if dist[n.ID] < 0 {
				connected = false
				break
			}
		}

		if connected {
			if maxDist > diameter {
				diameter = maxDist
			}
			if minEccentricity < 0 || maxDist < minEccentricity {
				minEccentricity = maxDist
			}
		}
	}

	if minEccentricity < 0 {
		minEccentricity = 0
	}

	return diameter, minEccentricity
}

// gonumCommunityDetection uses gonum's Louvain implementation for community detection.
func gonumCommunityDetection(gGraph *gonumGraph) (map[string]int, float64) {
	if len(gGraph.idToNodeID) == 0 {
		return make(map[string]int), 0
	}

	// Use gonum's Modularize (Louvain algorithm)
	// resolution=1.0 is standard modularity, src=nil uses default random
	reduced := community.Modularize(gGraph.undirected, 1.0, nil)

	// Extract communities from the reduced graph
	gonumCommunities := reduced.Communities()

	// Convert gonum communities to our format (node ID -> community index)
	communities := make(map[string]int)
	for communityIdx, comm := range gonumCommunities {
		for _, node := range comm {
			nodeID := gGraph.idToNodeID[node.ID()]
			communities[nodeID] = communityIdx
		}
	}

	// Calculate modularity using gonum's Q function
	modularity := community.Q(gGraph.undirected, gonumCommunities, 1.0)

	return communities, modularity
}

// Close releases analyzer resources.
func (a *GraphAnalyzer) Close() {
	a.parser.Close()
}

// DetectCycles uses gonum's Tarjan SCC to find cycles.
func (a *GraphAnalyzer) DetectCycles(graph *models.DependencyGraph) [][]string {
	if len(graph.Nodes) == 0 {
		return nil
	}

	gGraph := toGonumGraph(graph)
	gonumSCCs := topo.TarjanSCC(gGraph.directed)

	// Filter to only SCCs with more than one node (actual cycles)
	var sccs [][]string
	for _, scc := range gonumSCCs {
		if len(scc) > 1 {
			var nodeIDs []string
			for _, node := range scc {
				nodeIDs = append(nodeIDs, gGraph.idToNodeID[node.ID()])
			}
			sccs = append(sccs, nodeIDs)
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

	// Use gonum for PageRank
	gGraph := toGonumGraph(graph)
	pageRankMap := network.PageRank(gGraph.directed, 0.85, 1e-6)

	// Sort nodes by PageRank (highest first)
	ranked := make([]rankedNode, len(graph.Nodes))
	for i, node := range graph.Nodes {
		gonumID := gGraph.nodeIDToID[node.ID]
		ranked[i] = rankedNode{node: node, rank: pageRankMap[gonumID]}
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
