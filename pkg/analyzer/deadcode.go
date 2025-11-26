package analyzer

import (
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// DeadCodeAnalyzer detects unused functions, variables, and unreachable code.
type DeadCodeAnalyzer struct {
	parser     *parser.Parser
	confidence float64
}

// NewDeadCodeAnalyzer creates a new dead code analyzer.
func NewDeadCodeAnalyzer(confidence float64) *DeadCodeAnalyzer {
	if confidence <= 0 || confidence > 1 {
		confidence = 0.8
	}
	return &DeadCodeAnalyzer{
		parser:     parser.New(),
		confidence: confidence,
	}
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
	}

	collectDefinitions(result, fdc)
	collectUsages(result, fdc)

	return fdc, nil
}

type fileDeadCode struct {
	path        string
	definitions map[string]definition
	usages      map[string]bool
}

type definition struct {
	name       string
	kind       string // function, variable, class
	file       string
	line       uint32
	endLine    uint32
	visibility string
	exported   bool
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

	for _, fdc := range results {
		for name, def := range fdc.definitions {
			allDefs[name] = def
		}
		for name := range fdc.usages {
			allUsages[name] = true
		}
		analysis.Summary.TotalFilesAnalyzed++
	}

	// Find unused definitions
	for name, def := range allDefs {
		// Skip exported symbols (they might be used externally)
		if def.exported {
			continue
		}

		// Skip main functions
		if name == "main" || name == "init" {
			continue
		}

		if !allUsages[name] {
			confidence := a.calculateConfidence(def)
			if confidence >= a.confidence {
				if def.kind == "function" {
					analysis.DeadFunctions = append(analysis.DeadFunctions, models.DeadFunction{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						EndLine:    def.endLine,
						Visibility: def.visibility,
						Confidence: confidence,
						Reason:     "No references found in codebase",
					})
					analysis.Summary.AddDeadFunction(models.DeadFunction{File: def.file})
				} else if def.kind == "variable" {
					analysis.DeadVariables = append(analysis.DeadVariables, models.DeadVariable{
						Name:       name,
						File:       def.file,
						Line:       def.line,
						Confidence: confidence,
					})
					analysis.Summary.AddDeadVariable(models.DeadVariable{File: def.file})
				}
			}
		}
	}

	analysis.Summary.CalculatePercentage()
	return analysis, nil
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
