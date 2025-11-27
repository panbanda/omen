package analyzer

import (
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/RoaringBitmap/roaring/v2"
	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
	"github.com/zeebo/blake3"
)

// HierarchicalBitSet provides efficient bitmap-based reachability tracking using Roaring bitmaps.
// This matches PMAT's HierarchicalBitSet architecture for memory-efficient sparse bitset operations.
type HierarchicalBitSet struct {
	bitmap     *roaring.Bitmap
	totalNodes uint32
	mu         sync.RWMutex
}

// NewHierarchicalBitSet creates a new hierarchical bitset with the given capacity.
func NewHierarchicalBitSet(capacity uint32) *HierarchicalBitSet {
	return &HierarchicalBitSet{
		bitmap:     roaring.New(),
		totalNodes: capacity,
	}
}

// Set marks a node as reachable.
func (h *HierarchicalBitSet) Set(index uint32) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.bitmap.Add(index)
}

// IsSet checks if a node is reachable.
func (h *HierarchicalBitSet) IsSet(index uint32) bool {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return h.bitmap.Contains(index)
}

// CountSet returns the number of reachable nodes.
func (h *HierarchicalBitSet) CountSet() uint64 {
	h.mu.RLock()
	defer h.mu.RUnlock()
	return h.bitmap.GetCardinality()
}

// SetBatch marks multiple nodes as reachable efficiently.
func (h *HierarchicalBitSet) SetBatch(indices []uint32) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.bitmap.AddMany(indices)
}

// VTable represents a virtual method table for dynamic dispatch resolution.
type VTable struct {
	baseType string
	methods  map[string]uint32 // method name -> node ID
}

// VTableResolver resolves virtual method calls and interface implementations.
// This matches PMAT's VTableResolver architecture for accurate dead code detection in OOP languages.
type VTableResolver struct {
	vtables        map[string]*VTable  // type name -> vtable
	interfaceImpls map[string][]string // interface name -> implementing types
	mu             sync.RWMutex
}

// NewVTableResolver creates a new VTable resolver.
func NewVTableResolver() *VTableResolver {
	return &VTableResolver{
		vtables:        make(map[string]*VTable),
		interfaceImpls: make(map[string][]string),
	}
}

// RegisterType registers a type's virtual method table.
func (v *VTableResolver) RegisterType(typeName, baseType string, methods map[string]uint32) {
	v.mu.Lock()
	defer v.mu.Unlock()
	v.vtables[typeName] = &VTable{
		baseType: baseType,
		methods:  methods,
	}
}

// RegisterImplementation records that a type implements an interface.
func (v *VTableResolver) RegisterImplementation(interfaceName, typeName string) {
	v.mu.Lock()
	defer v.mu.Unlock()
	v.interfaceImpls[interfaceName] = append(v.interfaceImpls[interfaceName], typeName)
}

// ResolveDynamicCall returns all possible targets for a dynamic method call.
func (v *VTableResolver) ResolveDynamicCall(interfaceName, methodName string) []uint32 {
	v.mu.RLock()
	defer v.mu.RUnlock()

	var targets []uint32
	impls := v.interfaceImpls[interfaceName]
	for _, implType := range impls {
		if vtable, ok := v.vtables[implType]; ok {
			if nodeID, exists := vtable.methods[methodName]; exists {
				targets = append(targets, nodeID)
			}
		}
	}
	return targets
}

// CoverageData integrates test coverage data from external tools (go test -cover, llvm-cov, etc.).
// This matches PMAT's CoverageData architecture for improved dead code detection accuracy.
type CoverageData struct {
	CoveredLines    map[string]map[uint32]bool  // file -> line -> covered
	ExecutionCounts map[string]map[uint32]int64 // file -> line -> count
	mu              sync.RWMutex
}

// NewCoverageData creates an empty coverage data structure.
func NewCoverageData() *CoverageData {
	return &CoverageData{
		CoveredLines:    make(map[string]map[uint32]bool),
		ExecutionCounts: make(map[string]map[uint32]int64),
	}
}

// IsLineCovered checks if a line was covered during test execution.
func (c *CoverageData) IsLineCovered(file string, line uint32) bool {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if fileLines, ok := c.CoveredLines[file]; ok {
		return fileLines[line]
	}
	return false
}

// GetExecutionCount returns how many times a line was executed.
func (c *CoverageData) GetExecutionCount(file string, line uint32) int64 {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if fileCounts, ok := c.ExecutionCounts[file]; ok {
		return fileCounts[line]
	}
	return 0
}

// CrossLangReferenceGraph tracks references across language boundaries.
// This matches PMAT's CrossLangReferenceGraph architecture.
type CrossLangReferenceGraph struct {
	Edges     []models.ReferenceEdge
	Nodes     map[uint32]*models.ReferenceNode
	EdgeIndex map[uint32][]int // node -> edge indices (outgoing)
	mu        sync.RWMutex
}

// NewCrossLangReferenceGraph creates an initialized reference graph.
func NewCrossLangReferenceGraph() *CrossLangReferenceGraph {
	return &CrossLangReferenceGraph{
		Edges:     make([]models.ReferenceEdge, 0),
		Nodes:     make(map[uint32]*models.ReferenceNode),
		EdgeIndex: make(map[uint32][]int),
	}
}

// AddNode adds a node to the reference graph.
func (g *CrossLangReferenceGraph) AddNode(node *models.ReferenceNode) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.Nodes[node.ID] = node
}

// AddEdge adds an edge to the reference graph with indexing.
func (g *CrossLangReferenceGraph) AddEdge(edge models.ReferenceEdge) {
	g.mu.Lock()
	defer g.mu.Unlock()
	edgeIdx := len(g.Edges)
	g.Edges = append(g.Edges, edge)
	g.EdgeIndex[edge.From] = append(g.EdgeIndex[edge.From], edgeIdx)
}

// GetOutgoingEdges returns all edges originating from a node.
func (g *CrossLangReferenceGraph) GetOutgoingEdges(nodeID uint32) []models.ReferenceEdge {
	g.mu.RLock()
	defer g.mu.RUnlock()
	indices := g.EdgeIndex[nodeID]
	edges := make([]models.ReferenceEdge, len(indices))
	for i, idx := range indices {
		edges[i] = g.Edges[idx]
	}
	return edges
}

// DeadCodeAnalyzer detects unused functions, variables, and unreachable code.
// Architecture matches PMAT's DeadCodeAnalyzer with:
// - HierarchicalBitSet for efficient reachability tracking
// - CrossLangReferenceGraph for cross-language references
// - VTableResolver for dynamic dispatch resolution
// - CoverageData for test coverage integration
// - 4-phase analysis: build_reference_graph -> resolve_dynamic_calls -> mark_reachable -> classify_dead_code
type DeadCodeAnalyzer struct {
	parser *parser.Parser

	// Multi-level reachability (PMAT architecture)
	reachability *HierarchicalBitSet

	// Cross-language reference tracking (PMAT architecture)
	references *CrossLangReferenceGraph

	// Dynamic dispatch resolution (PMAT architecture)
	vtableResolver *VTableResolver

	// Test coverage integration (PMAT architecture)
	coverageData *CoverageData

	// Entry points tracking
	entryPoints map[uint32]bool
	entryMu     sync.RWMutex

	confidence  float64
	buildGraph  bool
	nodeCounter uint32
}

// DefaultCapacity for small to medium projects.
const DefaultCapacity = 100_000

// LargeCapacity for enterprise projects.
const LargeCapacity = 1_000_000

// NewDeadCodeAnalyzer creates a new dead code analyzer with PMAT-compatible architecture.
func NewDeadCodeAnalyzer(confidence float64) *DeadCodeAnalyzer {
	if confidence <= 0 || confidence > 1 {
		confidence = 0.8
	}
	return &DeadCodeAnalyzer{
		parser:         parser.New(),
		reachability:   NewHierarchicalBitSet(DefaultCapacity),
		references:     NewCrossLangReferenceGraph(),
		vtableResolver: NewVTableResolver(),
		coverageData:   nil, // Optional, set via WithCoverage
		entryPoints:    make(map[uint32]bool),
		confidence:     confidence,
		buildGraph:     true,
	}
}

// WithCallGraph enables or disables call graph-based analysis.
func (a *DeadCodeAnalyzer) WithCallGraph(enabled bool) *DeadCodeAnalyzer {
	a.buildGraph = enabled
	return a
}

// WithCoverage adds test coverage data for improved accuracy.
func (a *DeadCodeAnalyzer) WithCoverage(coverage *CoverageData) *DeadCodeAnalyzer {
	a.coverageData = coverage
	return a
}

// WithCapacity sets the initial node capacity.
func (a *DeadCodeAnalyzer) WithCapacity(capacity uint32) *DeadCodeAnalyzer {
	a.reachability = NewHierarchicalBitSet(capacity)
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
		path:              path,
		definitions:       make(map[string]definition),
		usages:            make(map[string]bool),
		calls:             make([]callReference, 0),
		typeImpls:         make([]typeImplementation, 0),
		language:          result.Language,
		unreachableBlocks: make([]models.UnreachableBlock, 0),
	}

	collectDefinitions(result, fdc)
	collectUsages(result, fdc)
	collectCalls(result, fdc)
	collectTypeImplementations(result, fdc)

	// Detect unreachable code blocks
	fdc.unreachableBlocks = detectUnreachableBlocks(result)

	return fdc, nil
}

type fileDeadCode struct {
	path              string
	definitions       map[string]definition
	usages            map[string]bool
	calls             []callReference
	typeImpls         []typeImplementation // For VTable resolution
	language          parser.Language
	unreachableBlocks []models.UnreachableBlock
}

type definition struct {
	name         string
	kind         string // function, variable, class, method
	file         string
	line         uint32
	endLine      uint32
	visibility   string
	exported     bool
	nodeID       uint32
	isFFI        bool
	isTestFile   bool
	contextHash  string
	receiverType string // For methods: the type this method belongs to
}

type callReference struct {
	caller   string
	callee   string
	line     uint32
	refType  models.ReferenceType
	receiver string // For method calls: the receiver type
}

type typeImplementation struct {
	typeName      string
	interfaceName string
	methods       []string
}

// collectDefinitions finds all function, variable, and class definitions.
func collectDefinitions(result *parser.ParseResult, fdc *fileDeadCode) {
	root := result.Tree.RootNode()
	inTestFile := isTestFile(result.Path)

	// Collect functions with FFI detection
	functions := parser.GetFunctions(result)
	for _, fn := range functions {
		// Check for FFI export by finding the function node
		isFFI := false
		receiverType := ""
		parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
			if isFunctionNode(node.Type(), result.Language) {
				if nameNode := getFunctionNameNode(node, result.Language); nameNode != nil {
					if parser.GetNodeText(nameNode, source) == fn.Name {
						isFFI = isFFIExported(node, source, result.Language)
						receiverType = extractReceiverType(node, source, result.Language)
						return false // Stop walking
					}
				}
			}
			return true
		})

		kind := "function"
		if receiverType != "" {
			kind = "method"
		}

		fdc.definitions[fn.Name] = definition{
			name:         fn.Name,
			kind:         kind,
			file:         result.Path,
			line:         fn.StartLine,
			endLine:      fn.EndLine,
			visibility:   getVisibility(fn.Name, result.Language),
			exported:     isExported(fn.Name, result.Language),
			isFFI:        isFFI,
			isTestFile:   inTestFile,
			contextHash:  computeContextHash(fn.Name, result.Path, fn.StartLine, kind),
			receiverType: receiverType,
		}
	}

	// Collect variables
	varTypes := getVariableNodeTypes(result.Language)
	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, vt := range varTypes {
			if node.Type() == vt {
				name := extractVarName(node, source, result.Language)
				if name != "" {
					line := node.StartPoint().Row + 1
					fdc.definitions[name] = definition{
						name:        name,
						kind:        "variable",
						file:        result.Path,
						line:        line,
						endLine:     node.EndPoint().Row + 1,
						visibility:  getVisibility(name, result.Language),
						exported:    isExported(name, result.Language),
						isTestFile:  inTestFile,
						contextHash: computeContextHash(name, result.Path, line, "variable"),
					}
				}
			}
		}
		return true
	})

	// Collect classes/types
	collectClasses(result, fdc, inTestFile)
}

// collectClasses finds class/struct/type definitions.
func collectClasses(result *parser.ParseResult, fdc *fileDeadCode, inTestFile bool) {
	root := result.Tree.RootNode()
	classTypes := getClassNodeTypes(result.Language)

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, ct := range classTypes {
			if node.Type() == ct {
				name := extractClassName(node, source, result.Language)
				if name != "" {
					line := node.StartPoint().Row + 1
					fdc.definitions[name] = definition{
						name:        name,
						kind:        "class",
						file:        result.Path,
						line:        line,
						endLine:     node.EndPoint().Row + 1,
						visibility:  getVisibility(name, result.Language),
						exported:    isExported(name, result.Language),
						isTestFile:  inTestFile,
						contextHash: computeContextHash(name, result.Path, line, "class"),
					}
				}
			}
		}
		return true
	})
}

// getClassNodeTypes returns AST node types for class/struct definitions.
func getClassNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangGo:
		return []string{"type_declaration", "type_spec"}
	case parser.LangRust:
		return []string{"struct_item", "enum_item", "trait_item"}
	case parser.LangPython:
		return []string{"class_definition"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"class_declaration", "interface_declaration"}
	case parser.LangJava, parser.LangCSharp:
		return []string{"class_declaration", "interface_declaration", "struct_declaration"}
	case parser.LangCPP:
		return []string{"class_specifier", "struct_specifier"}
	default:
		return []string{"class_declaration"}
	}
}

// extractClassName extracts the class/struct/type name.
func extractClassName(node *sitter.Node, source []byte, _ parser.Language) string {
	if nameNode := node.ChildByFieldName("name"); nameNode != nil {
		return parser.GetNodeText(nameNode, source)
	}
	// Try type_identifier for some languages
	for i := range int(node.ChildCount()) {
		child := node.Child(i)
		if child.Type() == "type_identifier" || child.Type() == "identifier" {
			return parser.GetNodeText(child, source)
		}
	}
	return ""
}

// extractReceiverType extracts the receiver type for method definitions (Go-style).
func extractReceiverType(node *sitter.Node, source []byte, lang parser.Language) string {
	if lang != parser.LangGo {
		return ""
	}
	// Look for receiver field in Go method declarations
	if receiver := node.ChildByFieldName("receiver"); receiver != nil {
		// Extract the type from the parameter list
		for i := range int(receiver.ChildCount()) {
			child := receiver.Child(i)
			if child.Type() == "parameter_declaration" {
				if typeNode := child.ChildByFieldName("type"); typeNode != nil {
					text := parser.GetNodeText(typeNode, source)
					// Remove pointer prefix if present
					text = strings.TrimPrefix(text, "*")
					return text
				}
			}
		}
	}
	return ""
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
			callee, receiver := extractCalleeWithReceiver(node, source, result.Language)
			if callee != "" && currentFunction != "" {
				refType := models.RefDirectCall
				if receiver != "" {
					refType = models.RefDynamicDispatch // Could be virtual call
				}
				fdc.calls = append(fdc.calls, callReference{
					caller:   currentFunction,
					callee:   callee,
					line:     node.StartPoint().Row + 1,
					refType:  refType,
					receiver: receiver,
				})
			}
		}

		// Detect imports
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

// collectTypeImplementations finds interface implementations for VTable resolution.
func collectTypeImplementations(result *parser.ParseResult, fdc *fileDeadCode) {
	// Language-specific implementation collection
	switch result.Language {
	case parser.LangGo:
		collectGoTypeImplementations(result, fdc)
	case parser.LangJava, parser.LangCSharp:
		collectOOPTypeImplementations(result, fdc)
	case parser.LangTypeScript:
		collectTSTypeImplementations(result, fdc)
	}
}

// collectGoTypeImplementations finds Go interface implementations.
func collectGoTypeImplementations(result *parser.ParseResult, fdc *fileDeadCode) {
	// Go uses structural typing - track methods per type
	root := result.Tree.RootNode()
	typeMethods := make(map[string][]string)

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		if node.Type() == "method_declaration" {
			if receiver := node.ChildByFieldName("receiver"); receiver != nil {
				receiverType := ""
				for i := range int(receiver.ChildCount()) {
					child := receiver.Child(i)
					if child.Type() == "parameter_declaration" {
						if typeNode := child.ChildByFieldName("type"); typeNode != nil {
							receiverType = strings.TrimPrefix(parser.GetNodeText(typeNode, source), "*")
						}
					}
				}
				if receiverType != "" {
					if nameNode := getFunctionNameNode(node, result.Language); nameNode != nil {
						methodName := parser.GetNodeText(nameNode, source)
						typeMethods[receiverType] = append(typeMethods[receiverType], methodName)
					}
				}
			}
		}
		return true
	})

	// Store for later VTable registration
	for typeName, methods := range typeMethods {
		fdc.typeImpls = append(fdc.typeImpls, typeImplementation{
			typeName: typeName,
			methods:  methods,
		})
	}
}

// collectOOPTypeImplementations finds Java/C# style interface implementations.
func collectOOPTypeImplementations(result *parser.ParseResult, fdc *fileDeadCode) {
	root := result.Tree.RootNode()

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		if node.Type() == "class_declaration" {
			className := extractClassName(node, source, result.Language)

			// Look for implements clause
			if implements := node.ChildByFieldName("interfaces"); implements != nil {
				for i := range int(implements.ChildCount()) {
					child := implements.Child(i)
					if child.Type() == "type_identifier" {
						interfaceName := parser.GetNodeText(child, source)
						fdc.typeImpls = append(fdc.typeImpls, typeImplementation{
							typeName:      className,
							interfaceName: interfaceName,
						})
					}
				}
			}
		}
		return true
	})
}

// collectTSTypeImplementations finds TypeScript interface implementations.
func collectTSTypeImplementations(result *parser.ParseResult, fdc *fileDeadCode) {
	root := result.Tree.RootNode()

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		if node.Type() == "class_declaration" {
			className := extractClassName(node, source, result.Language)

			// Look for implements clause in heritage
			if heritage := node.ChildByFieldName("heritage"); heritage != nil {
				parser.Walk(heritage, source, func(child *sitter.Node, _ []byte) bool {
					if child.Type() == "implements_clause" {
						for i := range int(child.ChildCount()) {
							typeNode := child.Child(i)
							if typeNode.Type() == "type_identifier" {
								interfaceName := parser.GetNodeText(typeNode, source)
								fdc.typeImpls = append(fdc.typeImpls, typeImplementation{
									typeName:      className,
									interfaceName: interfaceName,
								})
							}
						}
					}
					return true
				})
			}
		}
		return true
	})
}

// extractCalleeWithReceiver extracts the function name and receiver type being called.
func extractCalleeWithReceiver(node *sitter.Node, source []byte, _ parser.Language) (callee, receiver string) {
	if fnNode := node.ChildByFieldName("function"); fnNode != nil {
		// Handle method calls: obj.method() -> extract "method" and "obj" type
		if fnNode.Type() == "member_expression" || fnNode.Type() == "field_expression" ||
			fnNode.Type() == "selector_expression" {
			if propNode := fnNode.ChildByFieldName("property"); propNode != nil {
				callee = parser.GetNodeText(propNode, source)
			} else if fieldNode := fnNode.ChildByFieldName("field"); fieldNode != nil {
				callee = parser.GetNodeText(fieldNode, source)
			}
			// Try to get receiver
			if objNode := fnNode.ChildByFieldName("object"); objNode != nil {
				receiver = parser.GetNodeText(objNode, source)
			} else if objNode := fnNode.ChildByFieldName("operand"); objNode != nil {
				receiver = parser.GetNodeText(objNode, source)
			}
			return callee, receiver
		}
		return parser.GetNodeText(fnNode, source), ""
	}
	// Try first child for some languages
	if node.ChildCount() > 0 {
		firstChild := node.Child(0)
		if firstChild.Type() == "identifier" || firstChild.Type() == "scoped_identifier" {
			return parser.GetNodeText(firstChild, source), ""
		}
	}
	return "", ""
}

// isFunctionNode checks if a node type represents a function definition.
func isFunctionNode(nodeType string, _ parser.Language) bool {
	functionTypes := map[string]bool{
		"function_declaration":    true,
		"method_declaration":      true,
		"function_definition":     true,
		"function_item":           true,
		"method_definition":       true,
		"function":                true,
		"arrow_function":          true,
		"method":                  true,
		"constructor_declaration": true,
		"lambda_expression":       true,
	}
	return functionTypes[nodeType]
}

// detectUnreachableBlocks finds code blocks that cannot be reached.
func detectUnreachableBlocks(result *parser.ParseResult) []models.UnreachableBlock {
	var blocks []models.UnreachableBlock
	root := result.Tree.RootNode()

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		// Look for function bodies
		if isFunctionNode(node.Type(), result.Language) {
			bodyNode := getFunctionBody(node)
			if bodyNode != nil {
				unreachable := findUnreachableInBlock(bodyNode, source, result.Language, result.Path)
				blocks = append(blocks, unreachable...)
			}
		}
		return true
	})

	return blocks
}

// getFunctionBody returns the body node of a function.
func getFunctionBody(node *sitter.Node) *sitter.Node {
	// Try common field names
	if body := node.ChildByFieldName("body"); body != nil {
		return body
	}
	if body := node.ChildByFieldName("block"); body != nil {
		return body
	}
	// Look for block children
	for i := range int(node.ChildCount()) {
		child := node.Child(i)
		if child.Type() == "block" || child.Type() == "statement_block" ||
			child.Type() == "compound_statement" {
			return child
		}
	}
	return nil
}

// findUnreachableInBlock finds unreachable code within a block.
func findUnreachableInBlock(block *sitter.Node, source []byte, lang parser.Language, file string) []models.UnreachableBlock {
	var unreachable []models.UnreachableBlock

	// Track if we've seen a terminating statement
	seenTerminator := false
	terminatorLine := uint32(0)

	for i := range int(block.ChildCount()) {
		child := block.Child(i)
		nodeType := child.Type()

		// Skip non-statement nodes
		if nodeType == "{" || nodeType == "}" || nodeType == "comment" {
			continue
		}

		if seenTerminator {
			// This code is unreachable
			startLine := child.StartPoint().Row + 1
			endLine := child.EndPoint().Row + 1

			// Merge with previous block if consecutive
			if len(unreachable) > 0 {
				last := &unreachable[len(unreachable)-1]
				if last.EndLine+1 >= startLine {
					last.EndLine = endLine
					continue
				}
			}

			unreachable = append(unreachable, models.UnreachableBlock{
				File:      file,
				StartLine: startLine,
				EndLine:   endLine,
				Reason:    getUnreachableReason(terminatorLine),
			})
			continue
		}

		// Check if this is a terminating statement
		if isTerminatingStatement(child, source, lang) {
			seenTerminator = true
			terminatorLine = child.StartPoint().Row + 1
		}
	}

	return unreachable
}

// isTerminatingStatement checks if a statement terminates control flow.
func isTerminatingStatement(node *sitter.Node, source []byte, lang parser.Language) bool {
	nodeType := node.Type()

	// Return statements
	if nodeType == "return_statement" || nodeType == "return" {
		return true
	}

	// Language-specific terminators
	switch lang {
	case parser.LangGo:
		// panic() calls
		if nodeType == "expression_statement" || nodeType == "call_expression" {
			text := parser.GetNodeText(node, source)
			if strings.Contains(text, "panic(") || strings.Contains(text, "os.Exit(") ||
				strings.Contains(text, "log.Fatal") || strings.Contains(text, "log.Panic") {
				return true
			}
		}
	case parser.LangRust:
		// panic!(), unreachable!(), todo!(), unimplemented!()
		if nodeType == "expression_statement" || nodeType == "macro_invocation" {
			text := parser.GetNodeText(node, source)
			if strings.Contains(text, "panic!") || strings.Contains(text, "unreachable!") ||
				strings.Contains(text, "todo!") || strings.Contains(text, "unimplemented!") ||
				strings.Contains(text, "std::process::exit") {
				return true
			}
		}
	case parser.LangPython:
		// raise, sys.exit()
		if nodeType == "raise_statement" {
			return true
		}
		if nodeType == "expression_statement" {
			text := parser.GetNodeText(node, source)
			if strings.Contains(text, "sys.exit(") || strings.Contains(text, "os._exit(") ||
				strings.Contains(text, "exit(") || strings.Contains(text, "quit()") {
				return true
			}
		}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		// throw, process.exit()
		if nodeType == "throw_statement" {
			return true
		}
		if nodeType == "expression_statement" {
			text := parser.GetNodeText(node, source)
			if strings.Contains(text, "process.exit(") {
				return true
			}
		}
	case parser.LangJava, parser.LangCSharp:
		// throw
		if nodeType == "throw_statement" {
			return true
		}
		if nodeType == "expression_statement" {
			text := parser.GetNodeText(node, source)
			if strings.Contains(text, "System.exit(") || strings.Contains(text, "Environment.Exit(") {
				return true
			}
		}
	case parser.LangC, parser.LangCPP:
		// exit(), abort(), _Exit()
		if nodeType == "expression_statement" {
			text := parser.GetNodeText(node, source)
			if strings.Contains(text, "exit(") || strings.Contains(text, "abort(") ||
				strings.Contains(text, "_Exit(") || strings.Contains(text, "std::terminate") {
				return true
			}
		}
	}

	return false
}

// getUnreachableReason returns a human-readable reason for unreachable code.
func getUnreachableReason(terminatorLine uint32) string {
	return "Code after terminating statement at line " + string(rune(terminatorLine))
}

// getFunctionNameNode returns the name node of a function.
func getFunctionNameNode(node *sitter.Node, _ parser.Language) *sitter.Node {
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

// isTestFile checks if a file is a test file based on naming conventions.
func isTestFile(path string) bool {
	// Go test files
	if strings.HasSuffix(path, "_test.go") {
		return true
	}
	// Python test files (test_*.py or *_test.py)
	if strings.HasSuffix(path, "_test.py") || strings.Contains(path, "/test_") || strings.Contains(path, "\\test_") || strings.HasPrefix(filepath.Base(path), "test_") {
		return true
	}
	// JavaScript/TypeScript test files
	if strings.HasSuffix(path, ".test.ts") || strings.HasSuffix(path, ".test.js") ||
		strings.HasSuffix(path, ".spec.ts") || strings.HasSuffix(path, ".spec.js") ||
		strings.HasSuffix(path, ".test.tsx") || strings.HasSuffix(path, ".spec.tsx") {
		return true
	}
	// Rust test modules
	if strings.Contains(path, "/tests/") || strings.Contains(path, "\\tests\\") {
		return true
	}
	// Generic test directories
	if strings.Contains(path, "/test/") || strings.Contains(path, "\\test\\") {
		return true
	}
	return false
}

// isFFIExported checks if a function is exported via FFI (CGO, pyo3, etc.).
func isFFIExported(node *sitter.Node, source []byte, lang parser.Language) bool {
	switch lang {
	case parser.LangGo:
		// Check for //export comment before the function
		return hasGoExportComment(node, source)
	case parser.LangRust:
		// Check for #[no_mangle] or extern "C"
		return hasRustFFIAttribute(node, source)
	case parser.LangC, parser.LangCPP:
		// Check for __declspec(dllexport) or __attribute__((visibility("default")))
		return hasCFFIAttribute(node, source)
	case parser.LangPython:
		// Check for @pyo3.pyfunction or similar decorators
		return hasPythonFFIDecorator(node, source)
	default:
		return false
	}
}

// hasGoExportComment checks for //export comment preceding a Go function.
func hasGoExportComment(node *sitter.Node, source []byte) bool {
	startByte := node.StartByte()
	if startByte == 0 {
		return false
	}

	// Look backwards from the node to find comments
	searchStart := uint32(0)
	if startByte > 200 {
		searchStart = startByte - 200
	}

	precedingText := string(source[searchStart:startByte])
	lines := strings.Split(precedingText, "\n")

	// Check the last few lines for //export comment
	for i := len(lines) - 1; i >= 0 && i >= len(lines)-3; i-- {
		line := strings.TrimSpace(lines[i])
		if strings.HasPrefix(line, "//export ") {
			return true
		}
		// Also check for //go:linkname which marks FFI
		if strings.HasPrefix(line, "//go:linkname") {
			return true
		}
	}
	return false
}

// hasRustFFIAttribute checks for Rust FFI attributes.
func hasRustFFIAttribute(node *sitter.Node, source []byte) bool {
	startByte := node.StartByte()
	if startByte == 0 {
		return false
	}

	searchStart := uint32(0)
	if startByte > 200 {
		searchStart = startByte - 200
	}

	precedingText := string(source[searchStart:startByte])

	// Check for #[no_mangle]
	if strings.Contains(precedingText, "#[no_mangle]") {
		return true
	}
	// Check for extern "C"
	if strings.Contains(precedingText, "extern \"C\"") {
		return true
	}
	// Check for #[export_name]
	if strings.Contains(precedingText, "#[export_name") {
		return true
	}
	return false
}

// hasCFFIAttribute checks for C/C++ FFI attributes.
func hasCFFIAttribute(node *sitter.Node, source []byte) bool {
	nodeText := parser.GetNodeText(node, source)

	// Check for Windows DLL export
	if strings.Contains(nodeText, "__declspec(dllexport)") {
		return true
	}
	// Check for GCC visibility attribute
	if strings.Contains(nodeText, "__attribute__((visibility") {
		return true
	}
	// Check for extern "C"
	if strings.Contains(nodeText, "extern \"C\"") {
		return true
	}
	return false
}

// hasPythonFFIDecorator checks for Python FFI decorators.
func hasPythonFFIDecorator(node *sitter.Node, source []byte) bool {
	startByte := node.StartByte()
	if startByte == 0 {
		return false
	}

	searchStart := uint32(0)
	if startByte > 500 {
		searchStart = startByte - 500
	}

	precedingText := string(source[searchStart:startByte])

	// Check for pyo3 decorators
	if strings.Contains(precedingText, "@pyfunction") ||
		strings.Contains(precedingText, "@pyclass") ||
		strings.Contains(precedingText, "@pymethods") {
		return true
	}
	// Check for cffi decorators
	if strings.Contains(precedingText, "@ffi.def_extern") {
		return true
	}
	// Check for ctypes callback
	if strings.Contains(precedingText, "CFUNCTYPE") {
		return true
	}
	return false
}

// computeContextHash generates a BLAKE3 hash for deduplication.
func computeContextHash(name, file string, line uint32, kind string) string {
	data := name + ":" + file + ":" + string(rune(line)) + ":" + kind
	hash := blake3.Sum256([]byte(data))
	// Return first 16 hex characters
	return string(hash[:8])
}

// AnalyzeProject analyzes dead code across a project using the 4-phase PMAT architecture.
func (a *DeadCodeAnalyzer) AnalyzeProject(files []string) (*models.DeadCodeAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress analyzes dead code with optional progress callback.
// Implements PMAT's 4-phase analysis:
// Phase 1: Build reference graph from AST
// Phase 2: Resolve dynamic dispatch
// Phase 3: Mark reachable from entry points
// Phase 4: Classify dead code by type
func (a *DeadCodeAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress fileproc.ProgressFunc) (*models.DeadCodeAnalysis, error) {
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

	// Collect file results
	results := fileproc.MapFilesWithProgress(files, func(psr *parser.Parser, path string) (*fileDeadCode, error) {
		return analyzeFileDeadCode(psr, path)
	}, onProgress)

	allDefs := make(map[string]definition)
	allCalls := make([]callReference, 0)

	for _, fdc := range results {
		for name, def := range fdc.definitions {
			// Assign unique node ID
			def.nodeID = atomic.AddUint32(&a.nodeCounter, 1)
			allDefs[name] = def
		}
		allCalls = append(allCalls, fdc.calls...)

		// Register type implementations for VTable resolution
		for _, impl := range fdc.typeImpls {
			if impl.interfaceName != "" {
				a.vtableResolver.RegisterImplementation(impl.interfaceName, impl.typeName)
			}
		}

		// Collect unreachable blocks
		analysis.UnreachableCode = append(analysis.UnreachableCode, fdc.unreachableBlocks...)
		analysis.Summary.TotalUnreachableBlocks += len(fdc.unreachableBlocks)

		analysis.Summary.TotalFilesAnalyzed++
	}

	// Register methods in VTables
	a.registerMethodsInVTables(allDefs)

	if a.buildGraph {
		// Phase 1: Build reference graph from AST
		a.buildReferenceGraph(allDefs, allCalls)

		// Phase 2: Resolve dynamic dispatch
		a.resolveDynamicCalls()

		// Phase 3: Mark reachable from entry points (vectorized using Roaring)
		a.markReachableVectorized()

		// Phase 4: Classify dead code by type
		a.classifyDeadCode(analysis, allDefs)

		// Update summary with graph statistics
		analysis.Summary.TotalNodesInGraph = len(a.references.Nodes)
		analysis.Summary.ReachableNodes = int(a.reachability.CountSet())
	} else {
		// Fallback to simple usage-based analysis
		allUsages := make(map[string]bool)
		for _, fdc := range results {
			for name := range fdc.usages {
				allUsages[name] = true
			}
		}
		a.findDeadCodeFromUsages(analysis, allDefs, allUsages)
	}

	// Build legacy CallGraph for output compatibility
	analysis.CallGraph = a.buildLegacyCallGraph(allDefs, allCalls)

	analysis.Summary.CalculatePercentage()
	analysis.Summary.ConfidenceLevel = 0.85
	return analysis, nil
}

// registerMethodsInVTables registers methods in their type's VTable.
func (a *DeadCodeAnalyzer) registerMethodsInVTables(defs map[string]definition) {
	// Group methods by receiver type
	typeMethods := make(map[string]map[string]uint32)

	for _, def := range defs {
		if def.kind == "method" && def.receiverType != "" {
			if typeMethods[def.receiverType] == nil {
				typeMethods[def.receiverType] = make(map[string]uint32)
			}
			typeMethods[def.receiverType][def.name] = def.nodeID
		}
	}

	// Register VTables
	for typeName, methods := range typeMethods {
		a.vtableResolver.RegisterType(typeName, "", methods)
	}
}

// buildReferenceGraph constructs the cross-language reference graph (Phase 1).
func (a *DeadCodeAnalyzer) buildReferenceGraph(defs map[string]definition, calls []callReference) {
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
		a.references.AddNode(node)
		nameToNode[name] = def.nodeID

		// Track entry points
		if node.IsEntry {
			a.entryMu.Lock()
			a.entryPoints[def.nodeID] = true
			a.entryMu.Unlock()
		}
	}

	// Add edges from call references
	for _, call := range calls {
		callerID, callerExists := nameToNode[call.caller]
		calleeID, calleeExists := nameToNode[call.callee]

		if callerExists && calleeExists {
			a.references.AddEdge(models.ReferenceEdge{
				From:       callerID,
				To:         calleeID,
				Type:       call.refType,
				Confidence: 0.95,
			})
		}
	}
}

// resolveDynamicCalls resolves virtual method calls to concrete implementations (Phase 2).
func (a *DeadCodeAnalyzer) resolveDynamicCalls() {
	a.references.mu.Lock()
	defer a.references.mu.Unlock()

	// Find edges that are dynamic dispatch
	for _, edge := range a.references.Edges {
		if edge.Type == models.RefDynamicDispatch {
			// Get the callee node to find method name
			if node, ok := a.references.Nodes[edge.To]; ok {
				// Try to resolve to concrete implementations
				// This requires knowing the interface type, which we'd need from the receiver
				// For now, add edges to all possible implementations
				targets := a.vtableResolver.ResolveDynamicCall("", node.Name)
				for _, target := range targets {
					if target != edge.To {
						// Add edge to concrete implementation
						a.references.Edges = append(a.references.Edges, models.ReferenceEdge{
							From:       edge.From,
							To:         target,
							Type:       models.RefIndirectCall,
							Confidence: 0.7, // Lower confidence for resolved dynamic calls
						})
					}
				}
			}
		}
	}
}

// markReachableVectorized performs BFS using Roaring bitmap for efficiency (Phase 3).
func (a *DeadCodeAnalyzer) markReachableVectorized() {
	// Initialize with entry points
	a.entryMu.RLock()
	entryList := make([]uint32, 0, len(a.entryPoints))
	for nodeID := range a.entryPoints {
		entryList = append(entryList, nodeID)
	}
	a.entryMu.RUnlock()

	// Set all entry points as reachable
	a.reachability.SetBatch(entryList)

	// BFS traversal using queue
	queue := make([]uint32, len(entryList))
	copy(queue, entryList)

	for len(queue) > 0 {
		current := queue[0]
		queue = queue[1:]

		// Get all outgoing edges
		for _, edge := range a.references.GetOutgoingEdges(current) {
			if !a.reachability.IsSet(edge.To) {
				a.reachability.Set(edge.To)
				queue = append(queue, edge.To)
			}
		}
	}
}

// classifyDeadCode identifies dead code by type (Phase 4).
func (a *DeadCodeAnalyzer) classifyDeadCode(analysis *models.DeadCodeAnalysis, defs map[string]definition) {
	for name, def := range defs {
		// Skip entry points
		if isEntryPoint(name, def) {
			continue
		}

		// Check if node is unreachable
		if !a.reachability.IsSet(def.nodeID) {
			confidence := a.calculateConfidenceWithCoverage(def)
			if confidence >= a.confidence {
				switch def.kind {
				case "function", "method":
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

// calculateConfidenceWithCoverage uses coverage data to improve confidence.
func (a *DeadCodeAnalyzer) calculateConfidenceWithCoverage(def definition) float64 {
	confidence := 0.95 // Higher base confidence for graph-based analysis

	// Reduce confidence for exported symbols
	if def.exported {
		confidence -= 0.25
	}

	// Higher confidence for private symbols
	if def.visibility == "private" {
		confidence += 0.03
	}

	// Reduce confidence for test files
	if def.isTestFile {
		confidence -= 0.15
	}

	// Reduce confidence for FFI functions
	if def.isFFI {
		confidence -= 0.30
	}

	// Use coverage data if available
	if a.coverageData != nil {
		if a.coverageData.IsLineCovered(def.file, def.line) {
			// Code was executed in tests - much lower confidence it's dead
			confidence -= 0.40
		} else {
			// Code was never executed - higher confidence it's dead
			confidence += 0.05
		}
	}

	// Clamp to [0, 1]
	if confidence > 1.0 {
		confidence = 1.0
	}
	if confidence < 0.0 {
		confidence = 0.0
	}

	return confidence
}

// buildLegacyCallGraph builds a CallGraph for output compatibility.
func (a *DeadCodeAnalyzer) buildLegacyCallGraph(defs map[string]definition, calls []callReference) *models.CallGraph {
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
				if def.kind == "function" || def.kind == "method" {
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
				} else if def.kind == "class" {
					dc := models.DeadClass{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						EndLine:    def.endLine,
						Confidence: confidence,
						Reason:     "Class never used",
						Kind:       models.DeadKindClass,
					}
					analysis.DeadClasses = append(analysis.DeadClasses, dc)
					analysis.Summary.AddDeadClass(dc)
				}
			}
		}
	}
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

	// Reduce confidence for test files
	if def.isTestFile {
		confidence -= 0.15
	}

	// Reduce confidence for FFI functions
	if def.isFFI {
		confidence -= 0.25
	}

	// Clamp to [0, 1]
	if confidence > 1.0 {
		confidence = 1.0
	}
	if confidence < 0.0 {
		confidence = 0.0
	}

	return confidence
}

// isEntryPoint determines if a function is an entry point.
func isEntryPoint(name string, def definition) bool {
	// Standard entry points
	if name == "main" || name == "init" || name == "Main" {
		return true
	}
	// FFI exported functions are always entry points
	if def.isFFI {
		return true
	}
	// Exported symbols can be entry points
	if def.exported {
		return true
	}
	// Test functions (Go)
	if len(name) > 4 && (name[:4] == "Test" || name[:4] == "test") {
		return true
	}
	// Benchmark functions (Go)
	if len(name) > 9 && name[:9] == "Benchmark" {
		return true
	}
	// Example functions (Go)
	if len(name) > 7 && name[:7] == "Example" {
		return true
	}
	// Fuzz functions (Go 1.18+)
	if len(name) > 4 && name[:4] == "Fuzz" {
		return true
	}
	// HTTP handlers (common patterns)
	if isHTTPHandler(name) {
		return true
	}
	// Event handlers and callbacks
	if isEventHandler(name) {
		return true
	}
	// Lifecycle methods
	if isLifecycleMethod(name) {
		return true
	}
	return false
}

// isHTTPHandler checks if a function name matches HTTP handler patterns.
func isHTTPHandler(name string) bool {
	// Common HTTP handler patterns
	handlerSuffixes := []string{"Handler", "handler", "Endpoint", "endpoint", "Controller", "controller"}
	for _, suffix := range handlerSuffixes {
		if strings.HasSuffix(name, suffix) {
			return true
		}
	}
	// REST-style handlers
	httpVerbs := []string{"Get", "Post", "Put", "Delete", "Patch", "Head", "Options"}
	for _, verb := range httpVerbs {
		if strings.HasPrefix(name, verb) && len(name) > len(verb) {
			return true
		}
	}
	// Common handler method names
	if name == "ServeHTTP" || name == "Handle" || name == "serve" {
		return true
	}
	return false
}

// isEventHandler checks if a function name matches event handler patterns.
func isEventHandler(name string) bool {
	// Event handler patterns
	if strings.HasPrefix(name, "On") && len(name) > 2 {
		return true
	}
	if strings.HasPrefix(name, "on") && len(name) > 2 {
		return true
	}
	if strings.HasPrefix(name, "Handle") && len(name) > 6 {
		return true
	}
	if strings.HasPrefix(name, "handle") && len(name) > 6 {
		return true
	}
	// Callback patterns
	callbackSuffixes := []string{"Callback", "callback", "Listener", "listener", "Observer", "observer"}
	for _, suffix := range callbackSuffixes {
		if strings.HasSuffix(name, suffix) {
			return true
		}
	}
	return false
}

// isLifecycleMethod checks if a function name matches lifecycle method patterns.
func isLifecycleMethod(name string) bool {
	lifecycleMethods := []string{
		// Go
		"Setup", "Teardown", "TearDown", "SetUp",
		// Python
		"__init__", "__del__", "__enter__", "__exit__",
		"setUp", "tearDown", "setUpClass", "tearDownClass",
		// JavaScript/React
		"componentDidMount", "componentWillUnmount", "componentDidUpdate",
		"useEffect", "useState", "useMemo", "useCallback",
		// General
		"Initialize", "initialize", "Finalize", "finalize",
		"Start", "Stop", "Open", "Close",
		"Connect", "Disconnect", "Dispose",
	}
	for _, method := range lifecycleMethods {
		if name == method {
			return true
		}
	}
	return false
}

// Close releases analyzer resources.
func (a *DeadCodeAnalyzer) Close() {
	a.parser.Close()
}
