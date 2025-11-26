package analyzer

import (
	"sort"

	"github.com/jonathanreyes/omen-cli/pkg/models"
	"github.com/jonathanreyes/omen-cli/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// ComplexityAnalyzer computes cyclomatic and cognitive complexity.
type ComplexityAnalyzer struct {
	parser *parser.Parser
}

// NewComplexityAnalyzer creates a new complexity analyzer.
func NewComplexityAnalyzer() *ComplexityAnalyzer {
	return &ComplexityAnalyzer{
		parser: parser.New(),
	}
}

// AnalyzeFile analyzes complexity for a single file.
func (a *ComplexityAnalyzer) AnalyzeFile(path string) (*models.FileComplexity, error) {
	return analyzeFileComplexity(a.parser, path)
}

// countDecisionPoints counts branching statements for cyclomatic complexity.
func countDecisionPoints(node *sitter.Node, source []byte, lang parser.Language) uint32 {
	var count uint32

	decisionTypes := makeSet(getDecisionNodeTypes(lang))

	parser.WalkTyped(node, source, func(n *sitter.Node, nodeType string, src []byte) bool {
		if decisionTypes[nodeType] {
			count++
		}
		// Count logical operators (&&, ||) as additional decision points
		if nodeType == "binary_expression" || nodeType == "logical_expression" {
			op := getOperator(n, src)
			if op == "&&" || op == "||" || op == "and" || op == "or" {
				count++
			}
		}
		return true
	})

	return count
}

// makeSet converts a slice to a map for O(1) lookups.
func makeSet(items []string) map[string]bool {
	set := make(map[string]bool, len(items))
	for _, item := range items {
		set[item] = true
	}
	return set
}

// getDecisionNodeTypes returns AST node types that represent decision points.
func getDecisionNodeTypes(lang parser.Language) []string {
	// Common decision types across most languages
	common := []string{
		"if_statement",
		"if_expression",
		"while_statement",
		"while_expression",
		"for_statement",
		"for_expression",
		"case_statement",
		"catch_clause",
		"ternary_expression",
		"conditional_expression",
	}

	switch lang {
	case parser.LangGo:
		return append(common, "select_statement", "type_switch_statement", "expression_switch_statement")
	case parser.LangRust:
		return append(common, "match_expression", "loop_expression", "if_let_expression")
	case parser.LangPython:
		return append(common, "elif_clause", "except_clause", "with_statement", "comprehension")
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return append(common, "switch_statement", "do_statement")
	case parser.LangJava, parser.LangCSharp:
		return append(common, "switch_statement", "switch_expression", "do_statement", "enhanced_for_statement")
	case parser.LangC, parser.LangCPP:
		return append(common, "switch_statement", "do_statement")
	case parser.LangRuby:
		// Ruby uses different node names than most languages
		return []string{"if", "elsif", "unless", "while", "until", "for", "case", "when", "rescue", "conditional"}
	case parser.LangPHP:
		return append(common, "switch_statement", "elseif_clause")
	default:
		return common
	}
}

// getOperator extracts the operator from a binary expression node.
func getOperator(node *sitter.Node, source []byte) string {
	for i := range int(node.ChildCount()) {
		child := node.Child(i)
		if child.Type() == "&&" || child.Type() == "||" ||
			child.Type() == "and" || child.Type() == "or" {
			return child.Type()
		}
		// Some languages use operator field
		if child.IsNamed() && child.Type() == "operator" {
			return parser.GetNodeText(child, source)
		}
	}
	return ""
}

// cognitiveTypeInfo holds lookup maps for cognitive complexity calculation.
type cognitiveTypeInfo struct {
	nesting map[string]bool // Types that increment nesting depth
	flat    map[string]bool // Types that add complexity without nesting
}

// buildCognitiveTypeInfo builds lookup maps from cognitive node types.
func buildCognitiveTypeInfo(lang parser.Language) cognitiveTypeInfo {
	types := getCognitiveNodeTypes(lang)
	info := cognitiveTypeInfo{
		nesting: make(map[string]bool),
		flat:    make(map[string]bool),
	}
	for _, ct := range types {
		if ct.incrementsNesting {
			info.nesting[ct.nodeType] = true
		} else {
			info.flat[ct.nodeType] = true
		}
	}
	return info
}

// calculateCognitiveComplexity computes cognitive complexity with nesting penalties.
func calculateCognitiveComplexity(node *sitter.Node, source []byte, lang parser.Language, depth int) uint32 {
	info := buildCognitiveTypeInfo(lang)
	return calcCognitiveRecursive(node, source, info, depth)
}

// calcCognitiveRecursive is the recursive helper that reuses type info.
func calcCognitiveRecursive(node *sitter.Node, source []byte, info cognitiveTypeInfo, depth int) uint32 {
	var complexity uint32

	for i := range int(node.ChildCount()) {
		child := node.Child(i)
		childType := child.Type() // Cache type once per child

		if info.nesting[childType] {
			// Nesting construct: add base + depth penalty, recurse with increased depth
			complexity++
			complexity += uint32(depth)
			complexity += calcCognitiveRecursive(child, source, info, depth+1)
		} else if info.flat[childType] {
			// Flat construct: add base + depth penalty, recurse at same depth
			complexity++
			complexity += uint32(depth)
			complexity += calcCognitiveRecursive(child, source, info, depth)
		} else {
			// Continue at same depth
			complexity += calcCognitiveRecursive(child, source, info, depth)
		}
	}

	return complexity
}

type cognitiveNodeType struct {
	nodeType          string
	incrementsNesting bool
}

// getCognitiveNodeTypes returns node types that add cognitive complexity.
func getCognitiveNodeTypes(lang parser.Language) []cognitiveNodeType {
	var nesting, flat []string

	switch lang {
	case parser.LangRuby:
		nesting = []string{"if", "unless", "while", "until", "for", "case", "begin"}
		flat = []string{"elsif", "else", "when", "rescue", "break", "next", "redo"}
	default:
		nesting = []string{
			"if_statement", "if_expression",
			"while_statement", "while_expression",
			"for_statement", "for_expression",
			"switch_statement", "match_expression",
			"try_statement",
		}
		flat = []string{
			"else_clause", "elif_clause", "elseif_clause",
			"break_statement", "continue_statement",
			"goto_statement",
		}
	}

	var types []cognitiveNodeType
	for _, t := range nesting {
		types = append(types, cognitiveNodeType{t, true})
	}
	for _, t := range flat {
		types = append(types, cognitiveNodeType{t, false})
	}

	return types
}

// nestingTypesSet is a pre-computed set for max nesting calculation.
var nestingTypesSet = makeSet([]string{
	"if_statement", "if_expression", "if", "unless",
	"while_statement", "while_expression", "while", "until",
	"for_statement", "for_expression", "for",
	"switch_statement", "match_expression", "case",
	"try_statement", "begin",
	"block", "body_statement",
	"function_definition", "function_declaration", "method",
	"lambda_expression", "arrow_function",
})

// calculateMaxNesting finds the maximum nesting depth.
func calculateMaxNesting(node *sitter.Node, source []byte, currentDepth int) int {
	maxDepth := currentDepth

	for i := range int(node.ChildCount()) {
		child := node.Child(i)
		childType := child.Type() // Cache type once

		var childMax int
		if nestingTypesSet[childType] {
			childMax = calculateMaxNesting(child, source, currentDepth+1)
		} else {
			childMax = calculateMaxNesting(child, source, currentDepth)
		}

		if childMax > maxDepth {
			maxDepth = childMax
		}
	}

	return maxDepth
}

// AnalyzeProject analyzes all files in a project using parallel processing.
func (a *ComplexityAnalyzer) AnalyzeProject(files []string) (*models.ComplexityAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress analyzes all files with optional progress callback.
func (a *ComplexityAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress ProgressFunc) (*models.ComplexityAnalysis, error) {
	results := MapFilesWithProgress(files, func(psr *parser.Parser, path string) (models.FileComplexity, error) {
		fc, err := analyzeFileComplexity(psr, path)
		if err != nil {
			return models.FileComplexity{}, err
		}
		return *fc, nil
	}, onProgress)

	analysis := &models.ComplexityAnalysis{Files: results}

	var totalCyc, totalCog uint32
	var totalFuncs int

	for _, fc := range results {
		totalCyc += fc.TotalCyclomatic
		totalCog += fc.TotalCognitive
		totalFuncs += len(fc.Functions)
	}

	analysis.Summary.TotalFiles = len(results)
	analysis.Summary.TotalFunctions = totalFuncs
	if totalFuncs > 0 {
		analysis.Summary.AvgCyclomatic = float64(totalCyc) / float64(totalFuncs)
		analysis.Summary.AvgCognitive = float64(totalCog) / float64(totalFuncs)
	}

	// Collect all function metrics for percentile calculation
	allCyclomatic := make([]uint32, 0, totalFuncs)
	allCognitive := make([]uint32, 0, totalFuncs)

	for _, fc := range results {
		for _, fn := range fc.Functions {
			allCyclomatic = append(allCyclomatic, fn.Metrics.Cyclomatic)
			allCognitive = append(allCognitive, fn.Metrics.Cognitive)

			if fn.Metrics.Cyclomatic > analysis.Summary.MaxCyclomatic {
				analysis.Summary.MaxCyclomatic = fn.Metrics.Cyclomatic
			}
			if fn.Metrics.Cognitive > analysis.Summary.MaxCognitive {
				analysis.Summary.MaxCognitive = fn.Metrics.Cognitive
			}
		}
	}

	// Calculate percentiles
	if len(allCyclomatic) > 0 {
		sort.Slice(allCyclomatic, func(i, j int) bool { return allCyclomatic[i] < allCyclomatic[j] })
		sort.Slice(allCognitive, func(i, j int) bool { return allCognitive[i] < allCognitive[j] })

		analysis.Summary.P50Cyclomatic = percentile(allCyclomatic, 50)
		analysis.Summary.P95Cyclomatic = percentile(allCyclomatic, 95)
		analysis.Summary.P50Cognitive = percentile(allCognitive, 50)
		analysis.Summary.P95Cognitive = percentile(allCognitive, 95)
	}

	return analysis, nil
}

// percentile calculates the p-th percentile of a sorted slice.
func percentile(sorted []uint32, p int) uint32 {
	if len(sorted) == 0 {
		return 0
	}
	idx := (p * len(sorted)) / 100
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

// analyzeFileComplexity analyzes a single file with the provided parser.
func analyzeFileComplexity(psr *parser.Parser, path string) (*models.FileComplexity, error) {
	result, err := psr.ParseFile(path)
	if err != nil {
		return nil, err
	}

	fc := &models.FileComplexity{
		Path:      path,
		Language:  string(result.Language),
		Functions: make([]models.FunctionComplexity, 0),
	}

	functions := parser.GetFunctions(result)
	for _, fn := range functions {
		fnComplexity := analyzeFunctionComplexity(fn, result)
		fc.Functions = append(fc.Functions, fnComplexity)
		fc.TotalCyclomatic += fnComplexity.Metrics.Cyclomatic
		fc.TotalCognitive += fnComplexity.Metrics.Cognitive
	}

	if len(fc.Functions) > 0 {
		fc.AvgCyclomatic = float64(fc.TotalCyclomatic) / float64(len(fc.Functions))
		fc.AvgCognitive = float64(fc.TotalCognitive) / float64(len(fc.Functions))
	}

	return fc, nil
}

// analyzeFunctionComplexity computes complexity metrics for a single function.
func analyzeFunctionComplexity(fn parser.FunctionNode, result *parser.ParseResult) models.FunctionComplexity {
	fc := models.FunctionComplexity{
		Name:      fn.Name,
		StartLine: fn.StartLine,
		EndLine:   fn.EndLine,
		Metrics:   models.ComplexityMetrics{},
	}

	if fn.Body == nil {
		fc.Metrics.Cyclomatic = 1
		return fc
	}

	fc.Metrics.Cyclomatic = 1 + countDecisionPoints(fn.Body, result.Source, result.Language)
	fc.Metrics.Cognitive = calculateCognitiveComplexity(fn.Body, result.Source, result.Language, 0)
	fc.Metrics.Lines = int(fn.EndLine - fn.StartLine + 1)
	fc.Metrics.MaxNesting = calculateMaxNesting(fn.Body, result.Source, 0)

	return fc
}

// Close releases analyzer resources.
func (a *ComplexityAnalyzer) Close() {
	a.parser.Close()
}
