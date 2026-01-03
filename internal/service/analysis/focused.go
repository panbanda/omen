package analysis

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/panbanda/omen/internal/locator"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/repomap"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/source"
)

// FocusedContextOptions configures focused context generation.
type FocusedContextOptions struct {
	Focus        string
	BaseDir      string
	RepoMap      *repomap.Map
	IncludeGraph bool // Include callers/callees from dependency graph
}

// FocusedContextResult contains the focused context for a file or symbol.
type FocusedContextResult struct {
	Target     FocusedTarget
	Complexity *ComplexityInfo
	SATD       []SATDItem
	Candidates []FocusedCandidate // For ambiguous matches
	CallGraph  *CallGraphInfo     // Callers and callees (when IncludeGraph=true)
}

// CallGraphInfo contains caller/callee information for code navigation.
type CallGraphInfo struct {
	Callers       []CallRef  // Functions that call the focused function
	Callees       []CallRef  // Functions called by the focused function
	InternalCalls []CallEdge // For file focus: calls within the file
}

// CallRef represents a function reference in the call graph.
type CallRef struct {
	Name string // Function name
	File string // File path
	Line int    // Line number
	Kind string // "function", "method", etc.
}

// CallEdge represents a call relationship between two functions.
type CallEdge struct {
	From CallRef
	To   CallRef
}

// FocusedTarget identifies the resolved target.
type FocusedTarget struct {
	Type   string         // "file" or "symbol"
	Path   string         // For files
	Symbol *FocusedSymbol // For symbols
}

// FocusedSymbol contains symbol details.
type FocusedSymbol struct {
	Name string
	Kind string
	File string
	Line int
}

// FocusedCandidate represents an ambiguous match option.
type FocusedCandidate struct {
	Path string
	Name string
	File string
	Line int
	Kind string
}

// ComplexityInfo contains complexity metrics.
type ComplexityInfo struct {
	CyclomaticTotal int
	CognitiveTotal  int
	TopFunctions    []FunctionComplexity
}

// FunctionComplexity contains per-function complexity.
type FunctionComplexity struct {
	Name       string
	Line       int
	Cyclomatic int
	Cognitive  int
}

// SATDItem contains a technical debt marker.
type SATDItem struct {
	Line     int
	Type     string
	Content  string
	Severity string
}

// FocusedContext generates deep context for a specific file or symbol.
func (s *Service) FocusedContext(ctx context.Context, opts FocusedContextOptions) (*FocusedContextResult, error) {
	baseDir := opts.BaseDir
	if baseDir == "" {
		baseDir = "."
	}

	// Resolve the focus target
	locatorResult, err := locator.Locate(opts.Focus, opts.RepoMap, locator.WithBaseDir(baseDir))
	if err != nil {
		// For ambiguous matches, return result with candidates
		if err == locator.ErrAmbiguousMatch && locatorResult != nil {
			candidates := make([]FocusedCandidate, len(locatorResult.Candidates))
			for i, c := range locatorResult.Candidates {
				candidates[i] = FocusedCandidate{
					Path: c.Path,
					Name: c.Name,
					File: c.File,
					Line: c.Line,
					Kind: c.Kind,
				}
			}
			return &FocusedContextResult{Candidates: candidates}, err
		}
		return nil, err
	}

	switch locatorResult.Type {
	case locator.TargetFile:
		return s.focusedContextForFile(ctx, locatorResult.Path, opts)
	case locator.TargetSymbol:
		return s.focusedContextForSymbol(ctx, locatorResult.Symbol, opts)
	default:
		return nil, fmt.Errorf("unrecognized target type: %s", locatorResult.Type)
	}
}

func (s *Service) focusedContextForFile(ctx context.Context, path string, opts FocusedContextOptions) (*FocusedContextResult, error) {
	result := &FocusedContextResult{
		Target: FocusedTarget{
			Type: "file",
			Path: path,
		},
	}

	// Get complexity metrics
	cxAnalyzer := complexity.New()
	defer cxAnalyzer.Close()

	cxResult, err := cxAnalyzer.Analyze(ctx, []string{path}, source.NewFilesystem())
	if err == nil && cxResult != nil {
		cxInfo := &ComplexityInfo{}
		for _, f := range cxResult.Files {
			if f.Path == path {
				for _, fn := range f.Functions {
					cxInfo.CyclomaticTotal += int(fn.Metrics.Cyclomatic)
					cxInfo.CognitiveTotal += int(fn.Metrics.Cognitive)
					cxInfo.TopFunctions = append(cxInfo.TopFunctions, FunctionComplexity{
						Name:       fn.Name,
						Line:       int(fn.StartLine),
						Cyclomatic: int(fn.Metrics.Cyclomatic),
						Cognitive:  int(fn.Metrics.Cognitive),
					})
				}
				break
			}
		}
		result.Complexity = cxInfo
	}

	// Get SATD markers
	satdAnalyzer := satd.New()
	defer satdAnalyzer.Close()

	satdResult, err := satdAnalyzer.Analyze(ctx, []string{path}, source.NewFilesystem())
	if err == nil && satdResult != nil {
		for _, item := range satdResult.Items {
			if item.File == path || filepath.Base(item.File) == filepath.Base(path) {
				result.SATD = append(result.SATD, SATDItem{
					Line:     int(item.Line),
					Type:     item.Marker,
					Content:  item.Description,
					Severity: string(item.Severity),
				})
			}
		}
	}

	// Get call graph if requested
	if opts.IncludeGraph {
		result.CallGraph = s.buildCallGraphForFile(ctx, path, opts.BaseDir)
	}

	return result, nil
}

func (s *Service) focusedContextForSymbol(ctx context.Context, sym *locator.Symbol, opts FocusedContextOptions) (*FocusedContextResult, error) {
	result := &FocusedContextResult{
		Target: FocusedTarget{
			Type: "symbol",
			Symbol: &FocusedSymbol{
				Name: sym.Name,
				Kind: sym.Kind,
				File: sym.File,
				Line: sym.Line,
			},
		},
	}

	// Get complexity for the symbol's file
	cxAnalyzer := complexity.New()
	defer cxAnalyzer.Close()

	cxResult, err := cxAnalyzer.Analyze(ctx, []string{sym.File}, source.NewFilesystem())
	if err == nil && cxResult != nil {
		cxInfo := &ComplexityInfo{}
		for _, f := range cxResult.Files {
			for _, fn := range f.Functions {
				// Find the specific function
				if fn.Name == sym.Name && int(fn.StartLine) == sym.Line {
					cxInfo.CyclomaticTotal = int(fn.Metrics.Cyclomatic)
					cxInfo.CognitiveTotal = int(fn.Metrics.Cognitive)
					cxInfo.TopFunctions = []FunctionComplexity{{
						Name:       fn.Name,
						Line:       int(fn.StartLine),
						Cyclomatic: int(fn.Metrics.Cyclomatic),
						Cognitive:  int(fn.Metrics.Cognitive),
					}}
					break
				}
			}
		}
		result.Complexity = cxInfo
	}

	// Get call graph if requested
	if opts.IncludeGraph {
		result.CallGraph = s.buildCallGraphForSymbol(ctx, sym, opts.BaseDir)
	}

	return result, nil
}

// buildCallGraphForFile builds internal call relationships within a file.
func (s *Service) buildCallGraphForFile(ctx context.Context, path string, baseDir string) *CallGraphInfo {
	if baseDir == "" {
		baseDir = filepath.Dir(path)
	}

	// Scan for all Go files in the base directory
	files, err := s.scanSourceFiles(baseDir)
	if err != nil || len(files) == 0 {
		return nil
	}

	// Build the dependency graph at function scope
	graphAnalyzer := graph.New(graph.WithScope(graph.ScopeFunction))
	defer graphAnalyzer.Close()

	depGraph, err := graphAnalyzer.Analyze(ctx, files, source.NewFilesystem())
	if err != nil || depGraph == nil {
		return nil
	}

	result := &CallGraphInfo{
		InternalCalls: make([]CallEdge, 0),
	}

	// Find internal calls within this file
	nodesByFile := make(map[string][]graph.Node)
	for _, node := range depGraph.Nodes {
		nodesByFile[node.File] = append(nodesByFile[node.File], node)
	}

	fileNodes := nodesByFile[path]
	nodeIDSet := make(map[string]bool)
	for _, n := range fileNodes {
		nodeIDSet[n.ID] = true
	}

	// Find edges where both endpoints are in this file
	for _, edge := range depGraph.Edges {
		if nodeIDSet[edge.From] && nodeIDSet[edge.To] {
			fromNode := findNodeByID(depGraph.Nodes, edge.From)
			toNode := findNodeByID(depGraph.Nodes, edge.To)
			if fromNode != nil && toNode != nil {
				result.InternalCalls = append(result.InternalCalls, CallEdge{
					From: CallRef{
						Name: fromNode.Name,
						File: fromNode.File,
						Line: int(fromNode.Line),
						Kind: string(fromNode.Type),
					},
					To: CallRef{
						Name: toNode.Name,
						File: toNode.File,
						Line: int(toNode.Line),
						Kind: string(toNode.Type),
					},
				})
			}
		}
	}

	return result
}

// buildCallGraphForSymbol builds caller/callee lists for a specific symbol.
func (s *Service) buildCallGraphForSymbol(ctx context.Context, sym *locator.Symbol, baseDir string) *CallGraphInfo {
	if baseDir == "" {
		baseDir = filepath.Dir(sym.File)
	}

	// Scan for all source files in the base directory
	files, err := s.scanSourceFiles(baseDir)
	if err != nil || len(files) == 0 {
		return nil
	}

	// Build the dependency graph at function scope
	graphAnalyzer := graph.New(graph.WithScope(graph.ScopeFunction))
	defer graphAnalyzer.Close()

	depGraph, err := graphAnalyzer.Analyze(ctx, files, source.NewFilesystem())
	if err != nil || depGraph == nil {
		return nil
	}

	result := &CallGraphInfo{
		Callers: make([]CallRef, 0),
		Callees: make([]CallRef, 0),
	}

	// Build a node ID for the target symbol
	// Graph node IDs are in format "filepath:functionName"
	targetNodeID := sym.File + ":" + sym.Name

	// Find callers (edges where To == targetNodeID)
	// Find callees (edges where From == targetNodeID)
	for _, edge := range depGraph.Edges {
		if edge.To == targetNodeID {
			fromNode := findNodeByID(depGraph.Nodes, edge.From)
			if fromNode != nil {
				result.Callers = append(result.Callers, CallRef{
					Name: fromNode.Name,
					File: fromNode.File,
					Line: int(fromNode.Line),
					Kind: string(fromNode.Type),
				})
			}
		}
		if edge.From == targetNodeID {
			toNode := findNodeByID(depGraph.Nodes, edge.To)
			if toNode != nil {
				result.Callees = append(result.Callees, CallRef{
					Name: toNode.Name,
					File: toNode.File,
					Line: int(toNode.Line),
					Kind: string(toNode.Type),
				})
			}
		}
	}

	// Also match by function name alone (for cross-file calls where path differs)
	for _, edge := range depGraph.Edges {
		// Extract function name from edge endpoints
		toName := extractFuncName(edge.To)
		fromName := extractFuncName(edge.From)

		if toName == sym.Name && edge.To != targetNodeID {
			fromNode := findNodeByID(depGraph.Nodes, edge.From)
			if fromNode != nil && !containsCaller(result.Callers, fromNode.Name, fromNode.File) {
				result.Callers = append(result.Callers, CallRef{
					Name: fromNode.Name,
					File: fromNode.File,
					Line: int(fromNode.Line),
					Kind: string(fromNode.Type),
				})
			}
		}
		if fromName == sym.Name && edge.From != targetNodeID {
			toNode := findNodeByID(depGraph.Nodes, edge.To)
			if toNode != nil && !containsCallee(result.Callees, toNode.Name, toNode.File) {
				result.Callees = append(result.Callees, CallRef{
					Name: toNode.Name,
					File: toNode.File,
					Line: int(toNode.Line),
					Kind: string(toNode.Type),
				})
			}
		}
	}

	return result
}

// scanSourceFiles returns all source files in a directory.
func (s *Service) scanSourceFiles(baseDir string) ([]string, error) {
	var files []string
	err := filepath.Walk(baseDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // Skip errors
		}
		if info.IsDir() {
			// Skip hidden directories and common non-source dirs
			name := info.Name()
			if strings.HasPrefix(name, ".") || name == "vendor" || name == "node_modules" {
				return filepath.SkipDir
			}
			return nil
		}
		// Check for source file extensions
		ext := filepath.Ext(path)
		switch ext {
		case ".go", ".py", ".js", ".ts", ".tsx", ".jsx", ".rb", ".java", ".rs", ".c", ".cpp", ".h", ".hpp", ".cs", ".php", ".sh":
			files = append(files, path)
		}
		return nil
	})
	return files, err
}

// findNodeByID finds a node by its ID in the graph.
func findNodeByID(nodes []graph.Node, id string) *graph.Node {
	for i := range nodes {
		if nodes[i].ID == id {
			return &nodes[i]
		}
	}
	return nil
}

// extractFuncName extracts the function name from a node ID like "path/to/file.go:FuncName".
func extractFuncName(nodeID string) string {
	if idx := strings.LastIndex(nodeID, ":"); idx >= 0 {
		return nodeID[idx+1:]
	}
	return nodeID
}

// containsCaller checks if a caller is already in the list.
func containsCaller(callers []CallRef, name, file string) bool {
	for _, c := range callers {
		if c.Name == name && c.File == file {
			return true
		}
	}
	return false
}

// containsCallee checks if a callee is already in the list.
func containsCallee(callees []CallRef, name, file string) bool {
	for _, c := range callees {
		if c.Name == name && c.File == file {
			return true
		}
	}
	return false
}
