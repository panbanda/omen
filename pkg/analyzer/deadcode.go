package analyzer

import (
	"sync/atomic"

	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// DeadCodeAnalyzer detects unused functions, variables, and unreachable code.
type DeadCodeAnalyzer struct {
	parser       *parser.Parser
	confidence   float64
	buildGraph   bool // Whether to build and use call graph for analysis
	nodeCounter  uint32
}

// NewDeadCodeAnalyzer creates a new dead code analyzer.
func NewDeadCodeAnalyzer(confidence float64) *DeadCodeAnalyzer {
	if confidence <= 0 || confidence > 1 {
		confidence = 0.8
	}
	return &DeadCodeAnalyzer{
		parser:     parser.New(),
		confidence: confidence,
		buildGraph: true, // Enable call graph by default
	}
}

// WithCallGraph enables or disables call graph-based analysis.
func (a *DeadCodeAnalyzer) WithCallGraph(enabled bool) *DeadCodeAnalyzer {
	a.buildGraph = enabled
	return a
}

// AnalyzeFile analyzes a single file for dead code.
func (a *DeadCodeAnalyzer) AnalyzeFile(path string) (*fileDeadCode, error) {
	return analyzeFileDeadCode(a.parser, path)
}

// analyzeFileDeadCode analyzes a single file with the provided parser.
func analyzeFileDeadCode(psr *parser.Parser, path string) (*fileDeadCode, error) {
	result, err := psr.ParseFile(path)
	if err != nil {
		return nil, err
	}

	fdc := &fileDeadCode{
		path:        path,
		definitions: make(map[string]definition),
		usages:      make(map[string]bool),
		calls:       make([]callReference, 0),
		language:    result.Language,
	}

	collectDefinitions(result, fdc)
	collectUsages(result, fdc)
	collectCalls(result, fdc)

	return fdc, nil
}

type fileDeadCode struct {
	path        string
	definitions map[string]definition
	usages      map[string]bool
	calls       []callReference // Function calls within the file
	language    parser.Language
}

type definition struct {
	name       string
	kind       string // function, variable, class
	file       string
	line       uint32
	endLine    uint32
	visibility string
	exported   bool
	nodeID     uint32 // Unique node ID for call graph
}

type callReference struct {
	caller   string // Name of the calling function
	callee   string // Name of the called function
	line     uint32
	refType  models.ReferenceType
}

// collectDefinitions finds all function, variable, and class definitions.
func collectDefinitions(result *parser.ParseResult, fdc *fileDeadCode) {
	root := result.Tree.RootNode()

	// Collect functions
	functions := parser.GetFunctions(result)
	for _, fn := range functions {
		fdc.definitions[fn.Name] = definition{
			name:       fn.Name,
			kind:       "function",
			file:       result.Path,
			line:       fn.StartLine,
			endLine:    fn.EndLine,
			visibility: getVisibility(fn.Name, result.Language),
			exported:   isExported(fn.Name, result.Language),
		}
	}

	// Collect variables
	varTypes := getVariableNodeTypes(result.Language)
	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, vt := range varTypes {
			if node.Type() == vt {
				name := extractVarName(node, source, result.Language)
				if name != "" {
					fdc.definitions[name] = definition{
						name:       name,
						kind:       "variable",
						file:       result.Path,
						line:       node.StartPoint().Row + 1,
						endLine:    node.EndPoint().Row + 1,
						visibility: getVisibility(name, result.Language),
						exported:   isExported(name, result.Language),
					}
				}
			}
		}
		return true
	})
}

// collectUsages finds all identifier usages.
func collectUsages(result *parser.ParseResult, fdc *fileDeadCode) {
	root := result.Tree.RootNode()

	identTypes := []string{"identifier", "type_identifier", "field_identifier"}

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, it := range identTypes {
			if node.Type() == it {
				name := parser.GetNodeText(node, source)
				fdc.usages[name] = true
			}
		}
		// Also track call expressions
		if node.Type() == "call_expression" || node.Type() == "function_call" {
			if fnNode := node.ChildByFieldName("function"); fnNode != nil {
				name := parser.GetNodeText(fnNode, source)
				fdc.usages[name] = true
			}
		}
		return true
	})
}

// collectCalls extracts function call relationships for the call graph.
func collectCalls(result *parser.ParseResult, fdc *fileDeadCode) {
	root := result.Tree.RootNode()

	// Find the enclosing function for each call
	var currentFunction string

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		nodeType := node.Type()

		// Track which function we're inside
		if isFunctionNode(nodeType, result.Language) {
			if nameNode := getFunctionNameNode(node, result.Language); nameNode != nil {
				currentFunction = parser.GetNodeText(nameNode, source)
			}
		}

		// Detect function calls
		if nodeType == "call_expression" || nodeType == "function_call" || nodeType == "invocation_expression" {
			callee := extractCallee(node, source, result.Language)
			if callee != "" && currentFunction != "" {
				fdc.calls = append(fdc.calls, callReference{
					caller:  currentFunction,
					callee:  callee,
					line:    node.StartPoint().Row + 1,
					refType: models.RefDirectCall,
				})
			}
		}

		// Detect imports (for cross-file references)
		if nodeType == "import_declaration" || nodeType == "import_statement" ||
			nodeType == "use_declaration" || nodeType == "using_directive" {
			importName := extractImportName(node, source, result.Language)
			if importName != "" {
				fdc.calls = append(fdc.calls, callReference{
					caller:  "",
					callee:  importName,
					line:    node.StartPoint().Row + 1,
					refType: models.RefImport,
				})
			}
		}

		return true
	})
}

// isFunctionNode checks if a node type represents a function definition.
func isFunctionNode(nodeType string, lang parser.Language) bool {
	functionTypes := map[string]bool{
		"function_declaration":       true,
		"method_declaration":         true,
		"function_definition":        true,
		"function_item":              true,
		"method_definition":          true,
		"function":                   true,
		"arrow_function":             true,
		"method":                     true,
		"constructor_declaration":    true,
		"lambda_expression":          true,
	}
	return functionTypes[nodeType]
}

// getFunctionNameNode returns the name node of a function.
func getFunctionNameNode(node *sitter.Node, lang parser.Language) *sitter.Node {
	if nameNode := node.ChildByFieldName("name"); nameNode != nil {
		return nameNode
	}
	if nameNode := node.ChildByFieldName("declarator"); nameNode != nil {
		// C/C++ style: get the identifier from the declarator
		if identNode := nameNode.ChildByFieldName("declarator"); identNode != nil {
			return identNode
		}
		return nameNode
	}
	return nil
}

// extractCallee extracts the function name being called.
func extractCallee(node *sitter.Node, source []byte, lang parser.Language) string {
	if fnNode := node.ChildByFieldName("function"); fnNode != nil {
		// Handle method calls: obj.method() -> extract "method"
		if fnNode.Type() == "member_expression" || fnNode.Type() == "field_expression" {
			if propNode := fnNode.ChildByFieldName("property"); propNode != nil {
				return parser.GetNodeText(propNode, source)
			}
			if fieldNode := fnNode.ChildByFieldName("field"); fieldNode != nil {
				return parser.GetNodeText(fieldNode, source)
			}
		}
		return parser.GetNodeText(fnNode, source)
	}
	// Try first child for some languages
	if node.ChildCount() > 0 {
		firstChild := node.Child(0)
		if firstChild.Type() == "identifier" || firstChild.Type() == "scoped_identifier" {
			return parser.GetNodeText(firstChild, source)
		}
	}
	return ""
}

// extractImportName extracts the imported module/function name.
func extractImportName(node *sitter.Node, source []byte, lang parser.Language) string {
	switch lang {
	case parser.LangGo:
		// Go: import "path/package" -> extract package name
		if pathNode := node.ChildByFieldName("path"); pathNode != nil {
			text := parser.GetNodeText(pathNode, source)
			// Remove quotes and get last component
			text = text[1 : len(text)-1] // Remove quotes
			for i := len(text) - 1; i >= 0; i-- {
				if text[i] == '/' {
					return text[i+1:]
				}
			}
			return text
		}
	case parser.LangRust:
		// Rust: use path::item
		if pathNode := node.ChildByFieldName("argument"); pathNode != nil {
			return parser.GetNodeText(pathNode, source)
		}
	case parser.LangPython:
		// Python: from module import name or import module
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			return parser.GetNodeText(nameNode, source)
		}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		// JS/TS: import { name } from 'module'
		if sourceNode := node.ChildByFieldName("source"); sourceNode != nil {
			text := parser.GetNodeText(sourceNode, source)
			if len(text) > 2 {
				return text[1 : len(text)-1] // Remove quotes
			}
		}
	}
	return ""
}

// getVariableNodeTypes returns AST node types for variable declarations.
func getVariableNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangGo:
		return []string{"var_declaration", "const_declaration", "short_var_declaration"}
	case parser.LangRust:
		return []string{"let_declaration", "const_item", "static_item"}
	case parser.LangPython:
		return []string{"assignment", "augmented_assignment"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"variable_declaration", "lexical_declaration"}
	case parser.LangJava, parser.LangCSharp:
		return []string{"local_variable_declaration", "field_declaration"}
	case parser.LangC, parser.LangCPP:
		return []string{"declaration", "init_declarator"}
	case parser.LangRuby:
		return []string{"assignment"}
	case parser.LangPHP:
		return []string{"simple_variable", "property_declaration"}
	default:
		return []string{"variable_declaration", "assignment"}
	}
}

// extractVarName extracts variable name from a declaration node.
func extractVarName(node *sitter.Node, source []byte, lang parser.Language) string {
	switch lang {
	case parser.LangGo:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			return parser.GetNodeText(nameNode, source)
		}
		// For short_var_declaration, look for identifier_list
		for i := range int(node.ChildCount()) {
			child := node.Child(i)
			if child.Type() == "identifier" {
				return parser.GetNodeText(child, source)
			}
		}
	case parser.LangRust:
		if patternNode := node.ChildByFieldName("pattern"); patternNode != nil {
			return parser.GetNodeText(patternNode, source)
		}
	case parser.LangPython:
		if leftNode := node.ChildByFieldName("left"); leftNode != nil {
			return parser.GetNodeText(leftNode, source)
		}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		// Find the variable_declarator child
		for i := range int(node.ChildCount()) {
			child := node.Child(i)
			if child.Type() == "variable_declarator" {
				if nameNode := child.ChildByFieldName("name"); nameNode != nil {
					return parser.GetNodeText(nameNode, source)
				}
			}
		}
	default:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			return parser.GetNodeText(nameNode, source)
		}
	}
	return ""
}

// getVisibility determines if a symbol is public, private, or internal.
func getVisibility(name string, lang parser.Language) string {
	switch lang {
	case parser.LangGo:
		if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
			return "public"
		}
		return "private"
	case parser.LangRust:
		// Rust uses pub keyword, would need AST context
		return "unknown"
	case parser.LangPython:
		if len(name) > 1 && name[0] == '_' && name[1] == '_' {
			return "private"
		}
		if len(name) > 0 && name[0] == '_' {
			return "internal"
		}
		return "public"
	case parser.LangRuby:
		if len(name) > 0 && name[0] == '_' {
			return "private"
		}
		return "public"
	default:
		return "unknown"
	}
}

// isExported checks if a symbol is exported (can be used externally).
func isExported(name string, lang parser.Language) bool {
	switch lang {
	case parser.LangGo:
		return len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z'
	case parser.LangPython:
		return len(name) == 0 || name[0] != '_'
	default:
		return true // Conservative default
	}
}

// AnalyzeProject analyzes dead code across a project using parallel processing.
func (a *DeadCodeAnalyzer) AnalyzeProject(files []string) (*models.DeadCodeAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress analyzes dead code with optional progress callback.
func (a *DeadCodeAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress ProgressFunc) (*models.DeadCodeAnalysis, error) {
	analysis := &models.DeadCodeAnalysis{
		DeadFunctions:   make([]models.DeadFunction, 0),
		DeadVariables:   make([]models.DeadVariable, 0),
		DeadClasses:     make([]models.DeadClass, 0),
		UnreachableCode: make([]models.UnreachableBlock, 0),
		Summary:         models.NewDeadCodeSummary(),
	}

	if len(files) == 0 {
		return analysis, nil
	}

	results := MapFilesWithProgress(files, func(psr *parser.Parser, path string) (*fileDeadCode, error) {
		return analyzeFileDeadCode(psr, path)
	}, onProgress)

	allDefs := make(map[string]definition)
	allUsages := make(map[string]bool)
	allCalls := make([]callReference, 0)

	for _, fdc := range results {
		for name, def := range fdc.definitions {
			// Assign unique node ID
			def.nodeID = atomic.AddUint32(&a.nodeCounter, 1)
			allDefs[name] = def
		}
		for name := range fdc.usages {
			allUsages[name] = true
		}
		allCalls = append(allCalls, fdc.calls...)
		analysis.Summary.TotalFilesAnalyzed++
	}

	// Build call graph if enabled
	if a.buildGraph {
		callGraph := a.buildCallGraph(allDefs, allCalls)
		analysis.CallGraph = callGraph

		// Perform BFS reachability analysis
		reachable := a.computeReachability(callGraph)

		// Update summary with graph statistics
		analysis.Summary.TotalNodesInGraph = len(callGraph.Nodes)
		analysis.Summary.ReachableNodes = len(reachable)

		// Find dead code using reachability
		a.findDeadCodeFromGraph(analysis, allDefs, reachable)
	} else {
		// Fallback to simple usage-based analysis
		a.findDeadCodeFromUsages(analysis, allDefs, allUsages)
	}

	analysis.Summary.CalculatePercentage()
	analysis.Summary.ConfidenceLevel = 0.85 // Base confidence for graph-based analysis
	return analysis, nil
}

// buildCallGraph constructs a call graph from definitions and calls.
func (a *DeadCodeAnalyzer) buildCallGraph(defs map[string]definition, calls []callReference) *models.CallGraph {
	graph := models.NewCallGraph()

	// Create name to nodeID mapping
	nameToNode := make(map[string]uint32)

	// Add all definitions as nodes
	for name, def := range defs {
		node := &models.ReferenceNode{
			ID:         def.nodeID,
			Name:       name,
			File:       def.file,
			Line:       def.line,
			EndLine:    def.endLine,
			Kind:       def.kind,
			IsExported: def.exported,
			IsEntry:    isEntryPoint(name, def),
		}
		graph.AddNode(node)
		nameToNode[name] = def.nodeID
	}

	// Add edges from call references
	for _, call := range calls {
		callerID, callerExists := nameToNode[call.caller]
		calleeID, calleeExists := nameToNode[call.callee]

		if callerExists && calleeExists {
			graph.AddEdge(models.ReferenceEdge{
				From:       callerID,
				To:         calleeID,
				Type:       call.refType,
				Confidence: 0.95,
			})
		}
	}

	return graph
}

// isEntryPoint determines if a function is an entry point.
func isEntryPoint(name string, def definition) bool {
	// Standard entry points
	if name == "main" || name == "init" || name == "Main" {
		return true
	}
	// Exported symbols can be entry points
	if def.exported {
		return true
	}
	// Test functions
	if len(name) > 4 && (name[:4] == "Test" || name[:4] == "test") {
		return true
	}
	// Benchmark functions
	if len(name) > 9 && name[:9] == "Benchmark" {
		return true
	}
	return false
}

// computeReachability performs BFS from entry points to find reachable nodes.
func (a *DeadCodeAnalyzer) computeReachability(graph *models.CallGraph) map[uint32]bool {
	reachable := make(map[uint32]bool)

	// Initialize queue with entry points
	queue := make([]uint32, 0, len(graph.EntryPoints))
	for _, entry := range graph.EntryPoints {
		queue = append(queue, entry)
		reachable[entry] = true
	}

	// BFS traversal
	for len(queue) > 0 {
		current := queue[0]
		queue = queue[1:]

		// Get all outgoing edges
		for _, edge := range graph.GetOutgoingEdges(current) {
			if !reachable[edge.To] {
				reachable[edge.To] = true
				queue = append(queue, edge.To)
			}
		}
	}

	return reachable
}

// findDeadCodeFromGraph identifies dead code using graph reachability.
func (a *DeadCodeAnalyzer) findDeadCodeFromGraph(analysis *models.DeadCodeAnalysis, defs map[string]definition, reachable map[uint32]bool) {
	for name, def := range defs {
		// Skip entry points (already reachable)
		if isEntryPoint(name, def) {
			continue
		}

		// Check if node is unreachable
		if !reachable[def.nodeID] {
			confidence := a.calculateConfidenceFromGraph(def, reachable)
			if confidence >= a.confidence {
				switch def.kind {
				case "function":
					df := models.DeadFunction{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						EndLine:    def.endLine,
						Visibility: def.visibility,
						Confidence: confidence,
						Reason:     "Not reachable from any entry point",
						Kind:       models.DeadKindFunction,
						NodeID:     def.nodeID,
					}
					analysis.DeadFunctions = append(analysis.DeadFunctions, df)
					analysis.Summary.AddDeadFunction(df)
				case "class":
					dc := models.DeadClass{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						EndLine:    def.endLine,
						Confidence: confidence,
						Reason:     "Class never instantiated or referenced",
						Kind:       models.DeadKindClass,
						NodeID:     def.nodeID,
					}
					analysis.DeadClasses = append(analysis.DeadClasses, dc)
					analysis.Summary.AddDeadClass(dc)
				case "variable":
					dv := models.DeadVariable{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						Confidence: confidence,
						Reason:     "Variable never accessed",
						Kind:       models.DeadKindVariable,
						NodeID:     def.nodeID,
					}
					analysis.DeadVariables = append(analysis.DeadVariables, dv)
					analysis.Summary.AddDeadVariable(dv)
				}
			}
		}
	}
}

// findDeadCodeFromUsages uses simple usage tracking (fallback method).
func (a *DeadCodeAnalyzer) findDeadCodeFromUsages(analysis *models.DeadCodeAnalysis, defs map[string]definition, usages map[string]bool) {
	for name, def := range defs {
		// Skip exported symbols (they might be used externally)
		if def.exported {
			continue
		}

		// Skip main functions
		if name == "main" || name == "init" {
			continue
		}

		if !usages[name] {
			confidence := a.calculateConfidence(def)
			if confidence >= a.confidence {
				if def.kind == "function" {
					df := models.DeadFunction{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						EndLine:    def.endLine,
						Visibility: def.visibility,
						Confidence: confidence,
						Reason:     "No references found in codebase",
						Kind:       models.DeadKindFunction,
					}
					analysis.DeadFunctions = append(analysis.DeadFunctions, df)
					analysis.Summary.AddDeadFunction(df)
				} else if def.kind == "variable" {
					dv := models.DeadVariable{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						Confidence: confidence,
						Reason:     "Variable never used",
						Kind:       models.DeadKindVariable,
					}
					analysis.DeadVariables = append(analysis.DeadVariables, dv)
					analysis.Summary.AddDeadVariable(dv)
				}
			}
		}
	}
}

// calculateConfidenceFromGraph determines confidence using graph analysis.
func (a *DeadCodeAnalyzer) calculateConfidenceFromGraph(def definition, reachable map[uint32]bool) float64 {
	confidence := 0.95 // Higher base confidence for graph-based analysis

	// Reduce confidence for exported symbols
	if def.exported {
		confidence -= 0.25
	}

	// Higher confidence for private symbols
	if def.visibility == "private" {
		confidence += 0.03
	}

	// Cap at 1.0
	if confidence > 1.0 {
		confidence = 1.0
	}
	if confidence < 0.0 {
		confidence = 0.0
	}

	return confidence
}

// calculateConfidence determines how confident we are that code is dead.
func (a *DeadCodeAnalyzer) calculateConfidence(def definition) float64 {
	confidence := 0.9 // Base confidence for unused code

	// Reduce confidence for exported symbols
	if def.exported {
		confidence -= 0.3
	}

	// Higher confidence for private symbols
	if def.visibility == "private" {
		confidence += 0.05
	}

	// Cap at 1.0
	if confidence > 1.0 {
		confidence = 1.0
	}
	if confidence < 0.0 {
		confidence = 0.0
	}

	return confidence
}

// Close releases analyzer resources.
func (a *DeadCodeAnalyzer) Close() {
	a.parser.Close()
}
