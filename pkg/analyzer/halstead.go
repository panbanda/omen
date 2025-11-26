package analyzer

import (
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// HalsteadAnalyzer calculates Halstead software science metrics.
type HalsteadAnalyzer struct {
	operators map[string]int
	operands  map[string]int
}

// NewHalsteadAnalyzer creates a new Halstead analyzer.
func NewHalsteadAnalyzer() *HalsteadAnalyzer {
	return &HalsteadAnalyzer{
		operators: make(map[string]int),
		operands:  make(map[string]int),
	}
}

// Reset clears the analyzer state for a new analysis.
func (h *HalsteadAnalyzer) Reset() {
	h.operators = make(map[string]int)
	h.operands = make(map[string]int)
}

// AnalyzeNode analyzes a tree-sitter node for Halstead metrics.
func (h *HalsteadAnalyzer) AnalyzeNode(node *sitter.Node, source []byte, lang parser.Language) *models.HalsteadMetrics {
	h.Reset()
	h.walkNode(node, source, lang)

	operatorsUnique := uint32(len(h.operators))
	operandsUnique := uint32(len(h.operands))

	var operatorsTotal, operandsTotal uint32
	for _, count := range h.operators {
		operatorsTotal += uint32(count)
	}
	for _, count := range h.operands {
		operandsTotal += uint32(count)
	}

	return models.NewHalsteadMetrics(operatorsUnique, operandsUnique, operatorsTotal, operandsTotal)
}

// walkNode recursively walks the AST and classifies tokens.
func (h *HalsteadAnalyzer) walkNode(node *sitter.Node, source []byte, lang parser.Language) {
	if node == nil {
		return
	}

	nodeType := node.Type()
	text := parser.GetNodeText(node, source)

	// Classify based on node type and language
	if isOperatorNode(nodeType, text, lang) {
		h.operators[text]++
	} else if isOperandNode(nodeType, text, lang) {
		h.operands[text]++
	}

	// Recurse into children
	for i := range int(node.ChildCount()) {
		h.walkNode(node.Child(i), source, lang)
	}
}

// isOperatorNode determines if a node represents an operator.
func isOperatorNode(nodeType, text string, lang parser.Language) bool {
	// Common operator node types across languages
	operatorTypes := map[string]bool{
		// Binary operators
		"binary_expression":      true,
		"binary_operator":        true,
		"comparison_operator":    true,
		"assignment_expression":  true,
		"assignment_operator":    true,
		"augmented_assignment":   true,
		"compound_assignment":    true,
		"unary_expression":       true,
		"unary_operator":         true,
		"update_expression":      true,
		"logical_expression":     true,
		"boolean_operator":       true,
		"conditional_expression": true,
		"ternary_expression":     true,

		// Control flow (counted as operators)
		"if_statement":     true,
		"if_expression":    true,
		"if":               true, // Ruby
		"for_statement":    true,
		"for_expression":   true,
		"for":              true,
		"while_statement":  true,
		"while_expression": true,
		"while":            true,
		"switch_statement": true,
		"match_expression": true,
		"case":             true,
		"try_statement":    true,
		"catch_clause":     true,
		"except_clause":    true,
		"return_statement": true,
		"break_statement":  true,
		"continue":         true,

		// Function calls (the call itself is an operator)
		"call_expression": true,
		"call":            true,
		"method_call":     true,

		// Member access
		"member_expression":    true,
		"field_expression":     true,
		"selector_expression":  true,
		"subscript_expression": true,
		"index_expression":     true,
	}

	if operatorTypes[nodeType] {
		return true
	}

	// Check for operator symbols
	operatorSymbols := map[string]bool{
		"+": true, "-": true, "*": true, "/": true, "%": true,
		"=": true, "==": true, "!=": true, "<": true, ">": true,
		"<=": true, ">=": true, "&&": true, "||": true, "!": true,
		"&": true, "|": true, "^": true, "~": true,
		"<<": true, ">>": true, ">>>": true,
		"+=": true, "-=": true, "*=": true, "/=": true, "%=": true,
		"&=": true, "|=": true, "^=": true, "<<=": true, ">>=": true,
		"++": true, "--": true,
		"?": true, ":": true, // Ternary
		"=>": true, "->": true, // Arrow
		".": true, "::": true, // Member access
		"[": true, "]": true, // Subscript
		"(": true, ")": true, // Call/grouping
	}

	// Check if the text itself is an operator
	if operatorSymbols[text] {
		return true
	}

	// Language-specific keywords that are operators
	keywords := getOperatorKeywords(lang)
	return keywords[text]
}

// isOperandNode determines if a node represents an operand.
func isOperandNode(nodeType, text string, lang parser.Language) bool {
	// Operand node types
	operandTypes := map[string]bool{
		// Identifiers
		"identifier":       true,
		"type_identifier":  true,
		"field_identifier": true,

		// Literals
		"number":              true,
		"integer":             true,
		"integer_literal":     true,
		"float":               true,
		"float_literal":       true,
		"string":              true,
		"string_literal":      true,
		"interpreted_string":  true,
		"raw_string_literal":  true,
		"character":           true,
		"char_literal":        true,
		"boolean":             true,
		"true":                true,
		"false":               true,
		"nil":                 true,
		"null":                true,
		"none":                true, // Python
		"undefined":           true, // JavaScript
		"template_string":     true,
		"regex":               true,
		"regular_expression":  true,
		"array_literal":       true,
		"object_literal":      true,
		"tuple":               true,
		"list":                true,
		"dictionary":          true,
		"hash":                true,
		"interpreted_string_literal": true,
	}

	if operandTypes[nodeType] {
		return true
	}

	// Skip operators and punctuation
	if isOperatorNode(nodeType, text, lang) {
		return false
	}

	// Skip empty or whitespace-only text
	if len(text) == 0 {
		return false
	}

	// Skip common non-operand node types
	nonOperandTypes := map[string]bool{
		"source_file":             true,
		"program":                 true,
		"module":                  true,
		"package_clause":          true,
		"import_declaration":      true,
		"import_statement":        true,
		"function_declaration":    true,
		"function_definition":     true,
		"method_declaration":      true,
		"class_declaration":       true,
		"class_definition":        true,
		"block":                   true,
		"statement_block":         true,
		"expression_statement":    true,
		"comment":                 true,
		"line_comment":            true,
		"block_comment":           true,
		"parameter_list":          true,
		"parameters":              true,
		"argument_list":           true,
		"arguments":               true,
		"type_annotation":         true,
		"type_declaration":        true,
		"variable_declaration":    true,
		"short_var_declaration":   true,
		"const_declaration":       true,
		"let_declaration":         true,
		"var_declaration":         true,
		"lexical_declaration":     true,
		"formal_parameters":       true,
		"parenthesized_expression": true,
	}

	return !nonOperandTypes[nodeType]
}

// getOperatorKeywords returns language-specific operator keywords.
func getOperatorKeywords(lang parser.Language) map[string]bool {
	common := map[string]bool{
		"if": true, "else": true, "for": true, "while": true,
		"switch": true, "case": true, "default": true,
		"break": true, "continue": true, "return": true,
		"try": true, "catch": true, "finally": true, "throw": true,
		"new": true, "delete": true, "typeof": true, "instanceof": true,
		"in": true, "of": true,
	}

	switch lang {
	case parser.LangGo:
		common["go"] = true
		common["defer"] = true
		common["select"] = true
		common["range"] = true
		common["func"] = true
		common["chan"] = true
		common["make"] = true
		common["append"] = true
	case parser.LangRust:
		common["match"] = true
		common["loop"] = true
		common["impl"] = true
		common["trait"] = true
		common["async"] = true
		common["await"] = true
		common["move"] = true
		common["ref"] = true
		common["mut"] = true
	case parser.LangPython:
		common["elif"] = true
		common["except"] = true
		common["with"] = true
		common["as"] = true
		common["yield"] = true
		common["lambda"] = true
		common["and"] = true
		common["or"] = true
		common["not"] = true
		common["is"] = true
		common["pass"] = true
		common["raise"] = true
		common["assert"] = true
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		common["async"] = true
		common["await"] = true
		common["yield"] = true
		common["function"] = true
		common["class"] = true
		common["extends"] = true
		common["super"] = true
		common["this"] = true
		common["void"] = true
	case parser.LangJava, parser.LangCSharp:
		common["extends"] = true
		common["implements"] = true
		common["super"] = true
		common["this"] = true
		common["synchronized"] = true
		common["throws"] = true
	case parser.LangRuby:
		common["do"] = true
		common["end"] = true
		common["unless"] = true
		common["until"] = true
		common["begin"] = true
		common["rescue"] = true
		common["ensure"] = true
		common["yield"] = true
		common["defined?"] = true
	}

	return common
}
