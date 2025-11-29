package analyzer

import (
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// Pre-allocated maps for node type classification (avoids per-call allocation).

// halsteadOperatorTypes contains node types that represent operators.
var halsteadOperatorTypes = map[string]bool{
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
	"if":               true,
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

// halsteadOperatorSymbols contains symbols that are operators.
var halsteadOperatorSymbols = map[string]bool{
	"+": true, "-": true, "*": true, "/": true, "%": true,
	"=": true, "==": true, "!=": true, "<": true, ">": true,
	"<=": true, ">=": true, "&&": true, "||": true, "!": true,
	"&": true, "|": true, "^": true, "~": true,
	"<<": true, ">>": true, ">>>": true,
	"+=": true, "-=": true, "*=": true, "/=": true, "%=": true,
	"&=": true, "|=": true, "^=": true, "<<=": true, ">>=": true,
	"++": true, "--": true,
	"?": true, ":": true,
	"=>": true, "->": true,
	".": true, "::": true,
	"[": true, "]": true,
	"(": true, ")": true,
}

// halsteadOperandTypes contains node types that represent operands.
var halsteadOperandTypes = map[string]bool{
	// Identifiers
	"identifier":       true,
	"type_identifier":  true,
	"field_identifier": true,
	// Literals
	"number":                     true,
	"integer":                    true,
	"integer_literal":            true,
	"float":                      true,
	"float_literal":              true,
	"string":                     true,
	"string_literal":             true,
	"interpreted_string":         true,
	"raw_string_literal":         true,
	"character":                  true,
	"char_literal":               true,
	"boolean":                    true,
	"true":                       true,
	"false":                      true,
	"nil":                        true,
	"null":                       true,
	"none":                       true,
	"undefined":                  true,
	"template_string":            true,
	"regex":                      true,
	"regular_expression":         true,
	"array_literal":              true,
	"object_literal":             true,
	"tuple":                      true,
	"list":                       true,
	"dictionary":                 true,
	"hash":                       true,
	"interpreted_string_literal": true,
}

// halsteadNonOperandTypes contains node types that are not operands.
var halsteadNonOperandTypes = map[string]bool{
	"source_file":              true,
	"program":                  true,
	"module":                   true,
	"package_clause":           true,
	"import_declaration":       true,
	"import_statement":         true,
	"function_declaration":     true,
	"function_definition":      true,
	"method_declaration":       true,
	"class_declaration":        true,
	"class_definition":         true,
	"block":                    true,
	"statement_block":          true,
	"expression_statement":     true,
	"comment":                  true,
	"line_comment":             true,
	"block_comment":            true,
	"parameter_list":           true,
	"parameters":               true,
	"argument_list":            true,
	"arguments":                true,
	"type_annotation":          true,
	"type_declaration":         true,
	"variable_declaration":     true,
	"short_var_declaration":    true,
	"const_declaration":        true,
	"let_declaration":          true,
	"var_declaration":          true,
	"lexical_declaration":      true,
	"formal_parameters":        true,
	"parenthesized_expression": true,
}

// halsteadCommonKeywords contains common operator keywords.
var halsteadCommonKeywords = map[string]bool{
	"if": true, "else": true, "for": true, "while": true,
	"switch": true, "case": true, "default": true,
	"break": true, "continue": true, "return": true,
	"try": true, "catch": true, "finally": true, "throw": true,
	"new": true, "delete": true, "typeof": true, "instanceof": true,
	"in": true, "of": true,
}

// Language-specific operator keywords (pre-allocated).
var halsteadGoKeywords = map[string]bool{
	"go": true, "defer": true, "select": true, "range": true,
	"func": true, "chan": true, "make": true, "append": true,
}
var halsteadRustKeywords = map[string]bool{
	"match": true, "loop": true, "impl": true, "trait": true,
	"async": true, "await": true, "move": true, "ref": true, "mut": true,
}
var halsteadPythonKeywords = map[string]bool{
	"elif": true, "except": true, "with": true, "as": true,
	"yield": true, "lambda": true, "and": true, "or": true, "not": true,
	"is": true, "assert": true, "raise": true, "pass": true,
}

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
	if halsteadOperatorTypes[nodeType] {
		return true
	}

	if halsteadOperatorSymbols[text] {
		return true
	}

	// Check language-specific keywords
	if halsteadCommonKeywords[text] {
		return true
	}

	switch lang {
	case parser.LangGo:
		return halsteadGoKeywords[text]
	case parser.LangRust:
		return halsteadRustKeywords[text]
	case parser.LangPython:
		return halsteadPythonKeywords[text]
	}

	return false
}

// isOperandNode determines if a node represents an operand.
func isOperandNode(nodeType, text string, lang parser.Language) bool {
	if halsteadOperandTypes[nodeType] {
		return true
	}

	// Skip operators and punctuation
	if isOperatorNode(nodeType, text, lang) {
		return false
	}

	// Skip empty text
	if len(text) == 0 {
		return false
	}

	return !halsteadNonOperandTypes[nodeType]
}
