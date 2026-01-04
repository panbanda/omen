package graph

import (
	"context"
	"strings"
	"sync"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
	"gonum.org/v1/gonum/graph/community"
	"gonum.org/v1/gonum/graph/network"
	"gonum.org/v1/gonum/graph/path"
	"gonum.org/v1/gonum/graph/simple"
	"gonum.org/v1/gonum/graph/topo"
)

// Analyzer builds dependency graphs from source code.
type Analyzer struct {
	parser      *parser.Parser
	scope       Scope
	maxFileSize int64
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithScope sets the graph node granularity.
func WithScope(scope Scope) Option {
	return func(a *Analyzer) {
		a.scope = scope
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// New creates a new graph analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		parser:      parser.New(),
		scope:       ScopeFunction,
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// Compile-time check that Analyzer implements FileAnalyzer.
var _ analyzer.SourceFileAnalyzer[*DependencyGraph] = (*Analyzer)(nil)

// fileGraphData holds parsed graph data for a single file.
type fileGraphData struct {
	nodes     []Node
	imports   []string
	calls     map[string][]string // nodeID -> called functions
	classes   []string            // class/module names defined in this file
	classRefs []string            // class/constant references (for Ruby/Python)
	language  parser.Language
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

// extractClassDependencies extracts class/module definitions and references for Ruby/Python.
// Returns (defined classes, referenced classes).
func extractClassDependencies(result *parser.ParseResult) ([]string, []string) {
	var defined []string
	refsSet := make(map[string]bool)
	definedSet := make(map[string]bool)
	root := result.Tree.RootNode()

	switch result.Language {
	case parser.LangRuby:
		defined, definedSet = extractRubyClassDefinitions(root, result.Source)
		extractRubyClassReferences(root, result.Source, refsSet, definedSet)

	case parser.LangPython:
		defined, definedSet = extractPythonClassDefinitions(root, result.Source)
		extractPythonClassReferences(root, result.Source, refsSet, definedSet)
	}

	// Convert refs set to slice, excluding self-references
	var refs []string
	for ref := range refsSet {
		if !definedSet[ref] {
			refs = append(refs, ref)
		}
	}

	return defined, refs
}

// extractRubyClassDefinitions extracts class and module names defined in a Ruby file.
func extractRubyClassDefinitions(root *sitter.Node, source []byte) ([]string, map[string]bool) {
	var classes []string
	classSet := make(map[string]bool)

	parser.Walk(root, source, func(node *sitter.Node, src []byte) bool {
		nodeType := node.Type()
		if nodeType == "class" || nodeType == "module" {
			// Find the constant (class/module name) child
			for i := 0; i < int(node.ChildCount()); i++ {
				child := node.Child(i)
				if child.Type() == "constant" {
					name := parser.GetNodeText(child, src)
					if name != "" && !classSet[name] {
						classes = append(classes, name)
						classSet[name] = true
					}
					break
				}
				// Handle namespaced class: class Foo::Bar
				if child.Type() == "scope_resolution" {
					name := extractFullConstantName(child, src)
					if name != "" && !classSet[name] {
						classes = append(classes, name)
						classSet[name] = true
					}
					break
				}
			}
		}
		return true
	})

	return classes, classSet
}

// extractRubyClassReferences finds class/constant references in Ruby code.
func extractRubyClassReferences(root *sitter.Node, source []byte, refs map[string]bool, defined map[string]bool) {
	parser.Walk(root, source, func(node *sitter.Node, src []byte) bool {
		nodeType := node.Type()

		switch nodeType {
		case "superclass":
			// Inheritance: class Foo < Bar
			for i := 0; i < int(node.ChildCount()); i++ {
				child := node.Child(i)
				if child.Type() == "constant" {
					refs[parser.GetNodeText(child, src)] = true
				} else if child.Type() == "scope_resolution" {
					refs[extractFullConstantName(child, src)] = true
				}
			}

		case "call":
			// Check for include/extend/prepend
			var methodName string
			for i := 0; i < int(node.ChildCount()); i++ {
				child := node.Child(i)
				if child.Type() == "identifier" && i == 0 {
					methodName = parser.GetNodeText(child, src)
					break
				}
			}

			if methodName == "include" || methodName == "extend" || methodName == "prepend" {
				// Find the argument list and extract constants
				for i := 0; i < int(node.ChildCount()); i++ {
					child := node.Child(i)
					if child.Type() == "argument_list" {
						extractConstantsFromNode(child, src, refs)
					}
				}
			} else {
				// Method call on a constant: PaymentService.charge
				for i := 0; i < int(node.ChildCount()); i++ {
					child := node.Child(i)
					if child.Type() == "constant" && i == 0 {
						refs[parser.GetNodeText(child, src)] = true
						break
					}
					if child.Type() == "scope_resolution" && i == 0 {
						refs[extractFullConstantName(child, src)] = true
						break
					}
				}
			}

		case "constant":
			// Standalone constant reference (but skip if it's part of a class/module definition)
			parent := node.Parent()
			if parent != nil {
				parentType := parent.Type()
				// Skip constants that are class/module names being defined
				if parentType == "class" || parentType == "module" {
					// Check if this constant is the name being defined (first constant child)
					for i := 0; i < int(parent.ChildCount()); i++ {
						child := parent.Child(i)
						if child.Type() == "constant" {
							if child == node {
								return true // Skip - this is the definition
							}
							break
						}
					}
				}
			}
			refs[parser.GetNodeText(node, src)] = true
		}

		return true
	})
}

// extractPythonClassDefinitions extracts class names defined in a Python file.
func extractPythonClassDefinitions(root *sitter.Node, source []byte) ([]string, map[string]bool) {
	var classes []string
	classSet := make(map[string]bool)

	parser.Walk(root, source, func(node *sitter.Node, src []byte) bool {
		if node.Type() == "class_definition" {
			if nameNode := node.ChildByFieldName("name"); nameNode != nil {
				name := parser.GetNodeText(nameNode, src)
				if name != "" && !classSet[name] {
					classes = append(classes, name)
					classSet[name] = true
				}
			}
		}
		return true
	})

	return classes, classSet
}

// extractPythonClassReferences finds class references in Python code.
func extractPythonClassReferences(root *sitter.Node, source []byte, refs map[string]bool, defined map[string]bool) {
	parser.Walk(root, source, func(node *sitter.Node, src []byte) bool {
		nodeType := node.Type()

		switch nodeType {
		case "class_definition":
			// Check for base classes
			if basesNode := node.ChildByFieldName("superclasses"); basesNode != nil {
				extractIdentifiersFromNode(basesNode, src, refs)
			}

		case "call":
			// Class instantiation or method call: MyClass() or MyClass.method()
			if fnNode := node.ChildByFieldName("function"); fnNode != nil {
				// Direct call: MyClass()
				if fnNode.Type() == "identifier" {
					name := parser.GetNodeText(fnNode, src)
					// Heuristic: class names start with uppercase
					if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
						refs[name] = true
					}
				}
				// Attribute access: module.MyClass()
				if fnNode.Type() == "attribute" {
					if attrNode := fnNode.ChildByFieldName("attribute"); attrNode != nil {
						name := parser.GetNodeText(attrNode, src)
						if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
							refs[name] = true
						}
					}
				}
			}

		case "attribute":
			// SomeClass.class_method
			if objNode := node.ChildByFieldName("object"); objNode != nil {
				if objNode.Type() == "identifier" {
					name := parser.GetNodeText(objNode, src)
					if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
						refs[name] = true
					}
				}
			}
		}

		return true
	})
}

// extractFullConstantName extracts the full name from a scope_resolution node (e.g., "Stripe::Charge").
func extractFullConstantName(node *sitter.Node, source []byte) string {
	return parser.GetNodeText(node, source)
}

// extractConstantsFromNode extracts all constant names from a node tree.
func extractConstantsFromNode(node *sitter.Node, source []byte, refs map[string]bool) {
	parser.Walk(node, source, func(n *sitter.Node, src []byte) bool {
		if n.Type() == "constant" {
			refs[parser.GetNodeText(n, src)] = true
		} else if n.Type() == "scope_resolution" {
			refs[extractFullConstantName(n, src)] = true
			return false // Don't descend into scope_resolution children
		}
		return true
	})
}

// extractIdentifiersFromNode extracts identifier names from a node tree (for Python).
func extractIdentifiersFromNode(node *sitter.Node, source []byte, refs map[string]bool) {
	parser.Walk(node, source, func(n *sitter.Node, src []byte) bool {
		if n.Type() == "identifier" {
			name := parser.GetNodeText(n, src)
			// Heuristic: class names start with uppercase
			if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
				refs[name] = true
			}
		}
		return true
	})
}

// gonumGraph holds the gonum representation and mappings.
type gonumGraph struct {
	directed   *simple.DirectedGraph
	undirected *simple.UndirectedGraph
	nodeIDToID map[string]int64 // our node ID -> gonum int64 ID
	idToNodeID map[int64]string // gonum int64 ID -> our node ID
}

// toGonumGraph converts our DependencyGraph to gonum graph types.
func toGonumGraph(graph *DependencyGraph) *gonumGraph {
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
func sparsePageRank(nodes []Node, edges []Edge, damping float64, tolerance float64) map[string]float64 {
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
func (a *Analyzer) CalculatePageRankOnly(graph *DependencyGraph) *Metrics {
	metrics := &Metrics{
		NodeMetrics: make([]NodeMetric, 0, len(graph.Nodes)),
		Summary: Summary{
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
		nm := NodeMetric{
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
func (a *Analyzer) CalculateMetrics(graph *DependencyGraph) *Metrics {
	metrics := &Metrics{
		NodeMetrics: make([]NodeMetric, 0),
		Summary: Summary{
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
		nm := NodeMetric{
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
func calculateEigenvector(nodes []Node, inNeighbors map[string][]string, iterations int, tolerance float64) map[string]float64 {
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
func calculateClusteringCoefficients(nodes []Node, outNeighbors map[string][]string) map[string]float64 {
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
func calculateGlobalClustering(nodes []Node, outNeighbors map[string][]string) float64 {
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
func calculateAssortativity(graph *DependencyGraph) float64 {
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
func calculateReciprocity(graph *DependencyGraph) float64 {
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
func calculateDiameterAndRadius(nodes []Node, outNeighbors map[string][]string) (int, int) {
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

// ContentSource is an alias for analyzer.ContentSource.
type ContentSource = analyzer.ContentSource

// Analyze builds a dependency graph for a project from a ContentSource.
func (a *Analyzer) Analyze(ctx context.Context, files []string, src ContentSource) (*DependencyGraph, error) {
	// Get progress tracker from context
	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(files))
	}

	// Parse all files sequentially from ContentSource
	var fileData []fileGraphData
	for _, path := range files {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}

		if tracker != nil {
			tracker.Tick(path)
		}

		content, err := src.Read(path)
		if err != nil {
			continue
		}

		if a.maxFileSize > 0 && int64(len(content)) > a.maxFileSize {
			continue
		}

		fd, err := a.analyzeFileGraphFromContent(path, content)
		if err != nil {
			continue
		}
		fileData = append(fileData, fd)
	}

	return a.buildGraphFromFileData(fileData), nil
}

// analyzeFileGraphFromContent extracts graph data from file content.
func (a *Analyzer) analyzeFileGraphFromContent(path string, content []byte) (fileGraphData, error) {
	lang := parser.DetectLanguage(path)
	result, err := a.parser.Parse(content, lang, path)
	if err != nil {
		return fileGraphData{}, err
	}

	fd := fileGraphData{
		calls:    make(map[string][]string),
		language: result.Language,
	}

	switch a.scope {
	case ScopeFile:
		fd.nodes = append(fd.nodes, Node{
			ID:   path,
			Name: path,
			Type: NodeFile,
			File: path,
		})
		fd.imports = extractImports(result)

		// For Ruby/Python, extract class definitions and references
		if result.Language == parser.LangRuby || result.Language == parser.LangPython {
			fd.classes, fd.classRefs = extractClassDependencies(result)
		}

	case ScopeFunction:
		functions := parser.GetFunctions(result)
		for _, fn := range functions {
			nodeID := path + ":" + fn.Name
			node := Node{
				ID:   nodeID,
				Name: fn.Name,
				Type: NodeFunction,
				File: path,
				Line: fn.StartLine,
			}
			// Store signature in attributes if present
			if fn.Signature != "" {
				node.Attributes = map[string]string{
					"signature": fn.Signature,
				}
			}
			fd.nodes = append(fd.nodes, node)
			fd.calls[nodeID] = extractCalls(fn.Body, result.Source)
		}

	case ScopeModule:
		moduleName := extractModuleName(result)
		if moduleName != "" {
			fd.nodes = append(fd.nodes, Node{
				ID:   moduleName,
				Name: moduleName,
				Type: NodeModule,
				File: path,
			})
		}

		// For Ruby/Python, extract class dependencies for module-level edges
		if result.Language == parser.LangRuby || result.Language == parser.LangPython {
			if moduleName == "" {
				fd.nodes = append(fd.nodes, Node{
					ID:   path,
					Name: path,
					Type: NodeModule,
					File: path,
				})
			}
			fd.classes, fd.classRefs = extractClassDependencies(result)
		}
	}

	return fd, nil
}

// buildGraphFromFileData constructs the DependencyGraph from parsed file data.
func (a *Analyzer) buildGraphFromFileData(fileData []fileGraphData) *DependencyGraph {
	graph := NewDependencyGraph()
	nodeMap := make(map[string]bool)

	// Build index for O(1) function name lookup: funcName -> []nodeID
	funcNameIndex := make(map[string][]string)

	// Build class-to-file index for Ruby/Python dependency resolution
	classToFile := make(map[string]string)

	// Collect all nodes and build indices
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

		// Build class-to-file index from class definitions
		if len(fd.nodes) > 0 {
			filePath := fd.nodes[0].ID
			for _, className := range fd.classes {
				classToFile[className] = filePath
			}
		}
	}

	// Build edges using collected data
	var mu sync.Mutex
	for _, fd := range fileData {
		switch a.scope {
		case ScopeFile:
			// Traditional import-based edges
			for _, imp := range fd.imports {
				for targetPath := range nodeMap {
					if matchesImport(targetPath, imp) {
						mu.Lock()
						graph.AddEdge(Edge{
							From: fd.nodes[0].ID,
							To:   targetPath,
							Type: EdgeImport,
						})
						mu.Unlock()
					}
				}
			}

			// Class reference-based edges (for Ruby, Python, etc.)
			if len(fd.classRefs) > 0 && len(fd.nodes) > 0 {
				sourceFile := fd.nodes[0].ID
				for _, classRef := range fd.classRefs {
					if targetFile, ok := classToFile[classRef]; ok && targetFile != sourceFile {
						mu.Lock()
						graph.AddEdge(Edge{
							From: sourceFile,
							To:   targetFile,
							Type: EdgeReference,
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
								graph.AddEdge(Edge{
									From: sourceID,
									To:   targetID,
									Type: EdgeCall,
								})
							}
						}
						mu.Unlock()
					}
				}
			}

		case ScopeModule:
			// Class reference-based edges (for Ruby, Python, etc.)
			if len(fd.classRefs) > 0 && len(fd.nodes) > 0 {
				sourceFile := fd.nodes[0].ID
				for _, classRef := range fd.classRefs {
					if targetFile, ok := classToFile[classRef]; ok && targetFile != sourceFile {
						mu.Lock()
						graph.AddEdge(Edge{
							From: sourceFile,
							To:   targetFile,
							Type: EdgeReference,
						})
						mu.Unlock()
					}
				}
			}
		}
	}

	return graph
}

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	a.parser.Close()
}

// DetectCycles uses gonum's Tarjan SCC to find cycles.
func (a *Analyzer) DetectCycles(graph *DependencyGraph) [][]string {
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
	node Node
	rank float64
}

// PruneGraph reduces graph size while preserving important nodes using PageRank.
func (a *Analyzer) PruneGraph(graph *DependencyGraph, maxNodes, maxEdges int) *DependencyGraph {
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
	pruned := NewDependencyGraph()
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
