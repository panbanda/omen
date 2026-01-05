package parser

import (
	"context"

	sitter "github.com/smacker/go-tree-sitter"
)

// Inspector provides language-agnostic AST inspection capabilities.
// It abstracts the underlying AST implementation (tree-sitter, go/ast, etc.)
// to provide a unified interface for code analysis.
type Inspector interface {
	// GetFunctions extracts all function/method definitions.
	GetFunctions() []FunctionInfo

	// GetClasses extracts all class/struct/type definitions.
	GetClasses() []ClassInfo

	// GetImports extracts all import/use statements.
	GetImports() []ImportInfo

	// GetSymbols extracts all defined symbols (functions, classes, variables).
	// This is useful for dead code analysis where you need a complete symbol table.
	GetSymbols() []SymbolInfo

	// GetCallGraph extracts function call relationships.
	// Returns edges where each edge represents a call from caller to callee.
	GetCallGraph() []CallEdge

	// Language returns the detected language of the inspected code.
	Language() Language

	// Path returns the file path being inspected.
	Path() string
}

// SymbolKind classifies the type of symbol.
type SymbolKind string

const (
	SymbolFunction SymbolKind = "function"
	SymbolMethod   SymbolKind = "method"
	SymbolClass    SymbolKind = "class"
	SymbolStruct   SymbolKind = "struct"
	SymbolVariable SymbolKind = "variable"
	SymbolConstant SymbolKind = "constant"
	SymbolType     SymbolKind = "type"
)

// Visibility indicates symbol accessibility.
type Visibility string

const (
	VisibilityPublic   Visibility = "public"
	VisibilityPrivate  Visibility = "private"
	VisibilityInternal Visibility = "internal"
	VisibilityUnknown  Visibility = "unknown"
)

// Location represents a source code position.
type Location struct {
	File      string
	StartLine uint32
	EndLine   uint32
	StartCol  uint32
	EndCol    uint32
}

// FunctionInfo describes a function or method definition.
type FunctionInfo struct {
	Name         string
	Location     Location
	Signature    string     // Full signature without body
	Parameters   []string   // Parameter names
	ReceiverType string     // For methods: the type this method belongs to (empty for functions)
	Visibility   Visibility // public/private/internal
	IsExported   bool       // Language-specific export status
	IsAsync      bool       // async/await function
	IsGenerator  bool       // Generator function (yield)

	// Metadata for framework detection
	Decorators []string // Python/TS decorators, Java annotations
	IsTestFunc bool     // Test function (Test*, test_*, etc.)
	IsFFI      bool     // FFI exported (//export, #[no_mangle], etc.)
}

// ClassInfo describes a class, struct, interface, or type definition.
type ClassInfo struct {
	Name       string
	Location   Location
	Kind       string     // "class", "struct", "interface", "trait", "type"
	Visibility Visibility // public/private/internal
	IsExported bool       // Language-specific export status
	Extends    []string   // Base classes/types
	Implements []string   // Implemented interfaces
	Methods    []string   // Method names (for quick lookup)
	Fields     []string   // Field/property names
	Decorators []string   // Class-level decorators/annotations
}

// ImportInfo describes an import statement.
type ImportInfo struct {
	Module    string   // The imported module/package path
	Alias     string   // Local alias (empty if none)
	Names     []string // Specific imported names (empty for wildcard/default)
	IsDefault bool     // Default import (JS/TS)
	Location  Location
}

// SymbolInfo is a unified representation of any defined symbol.
type SymbolInfo struct {
	Name         string
	Kind         SymbolKind
	Location     Location
	Visibility   Visibility
	IsExported   bool
	ReceiverType string // For methods only

	// Raw underlying data for advanced use cases
	RawNode interface{} // *sitter.Node for tree-sitter, ast.Node for go/ast, etc.
}

// CallEdgeKind classifies the type of call relationship.
type CallEdgeKind string

const (
	CallDirect      CallEdgeKind = "direct"      // foo()
	CallMethod      CallEdgeKind = "method"      // obj.foo()
	CallIndirect    CallEdgeKind = "indirect"    // callback(fn)
	CallDynamic     CallEdgeKind = "dynamic"     // obj[method](), send(:method)
	CallConstructor CallEdgeKind = "constructor" // new Foo()
	CallSuperMethod CallEdgeKind = "super"       // super.foo()
)

// CallEdge represents a call from one function to another.
type CallEdge struct {
	CallerName   string       // Name of the calling function
	CalleeName   string       // Name of the called function
	Kind         CallEdgeKind // Type of call
	ReceiverType string       // For method calls: the receiver type/expression
	Location     Location     // Location of the call site
}

// InspectorFactory creates Inspectors from various sources.
type InspectorFactory interface {
	// FromFile creates an Inspector from a file path.
	FromFile(path string) (Inspector, error)

	// FromSource creates an Inspector from source code with known language.
	FromSource(source []byte, lang Language, path string) (Inspector, error)

	// FromParseResult creates an Inspector from an existing ParseResult.
	FromParseResult(result *ParseResult) Inspector
}

// TreeSitterInspector implements Inspector using tree-sitter.
// This is the default implementation wrapping the existing parser.
type TreeSitterInspector struct {
	result *ParseResult
	parser *Parser

	// Cached extraction results (lazy-loaded)
	functions []FunctionInfo
	classes   []ClassInfo
	imports   []ImportInfo
	symbols   []SymbolInfo
	callGraph []CallEdge
	extracted bool
}

// NewTreeSitterInspector creates an Inspector from a ParseResult.
func NewTreeSitterInspector(result *ParseResult) *TreeSitterInspector {
	return &TreeSitterInspector{
		result: result,
	}
}

// NewTreeSitterInspectorFactory creates a factory for tree-sitter based inspectors.
func NewTreeSitterInspectorFactory() InspectorFactory {
	return &treeSitterFactory{
		parser: New(),
	}
}

type treeSitterFactory struct {
	parser *Parser
}

func (f *treeSitterFactory) FromFile(path string) (Inspector, error) {
	result, err := f.parser.ParseFile(path)
	if err != nil {
		return nil, err
	}
	return NewTreeSitterInspector(result), nil
}

func (f *treeSitterFactory) FromSource(source []byte, lang Language, path string) (Inspector, error) {
	result, err := f.parser.Parse(source, lang, path)
	if err != nil {
		return nil, err
	}
	return NewTreeSitterInspector(result), nil
}

func (f *treeSitterFactory) FromParseResult(result *ParseResult) Inspector {
	return NewTreeSitterInspector(result)
}

// Language returns the detected language.
func (i *TreeSitterInspector) Language() Language {
	return i.result.Language
}

// Path returns the file path.
func (i *TreeSitterInspector) Path() string {
	return i.result.Path
}

// GetFunctions extracts all function definitions.
func (i *TreeSitterInspector) GetFunctions() []FunctionInfo {
	i.ensureExtracted()
	return i.functions
}

// GetClasses extracts all class/struct definitions.
func (i *TreeSitterInspector) GetClasses() []ClassInfo {
	i.ensureExtracted()
	return i.classes
}

// GetImports extracts all import statements.
func (i *TreeSitterInspector) GetImports() []ImportInfo {
	i.ensureExtracted()
	return i.imports
}

// GetSymbols extracts all defined symbols.
func (i *TreeSitterInspector) GetSymbols() []SymbolInfo {
	i.ensureExtracted()
	return i.symbols
}

// GetCallGraph extracts function call relationships.
func (i *TreeSitterInspector) GetCallGraph() []CallEdge {
	i.ensureExtracted()
	return i.callGraph
}

// ensureExtracted performs lazy extraction of all data in a single pass.
func (i *TreeSitterInspector) ensureExtracted() {
	if i.extracted {
		return
	}
	i.extracted = true

	root := i.result.Tree.RootNode()
	source := i.result.Source
	lang := i.result.Language

	// Extract everything in a single AST walk
	i.extractAll(root, source, lang)
}

// extractAll performs a single AST walk to extract all data.
func (i *TreeSitterInspector) extractAll(root *sitter.Node, source []byte, lang Language) {
	funcTypes := getFunctionNodeTypes(lang)
	classTypes := getClassNodeTypes(lang)
	importTypes := getImportNodeTypes(lang)
	varTypes := getVariableNodeTypes(lang)

	funcTypeSet := makeSet(funcTypes)
	classTypeSet := makeSet(classTypes)
	importTypeSet := makeSet(importTypes)
	varTypeSet := makeSet(varTypes)

	var currentFunction string

	WalkTyped(root, source, func(node *sitter.Node, nodeType string, src []byte) bool {
		// Functions
		if funcTypeSet[nodeType] {
			fn := i.extractFunctionInfo(node, src, lang)
			if fn != nil {
				i.functions = append(i.functions, *fn)
				i.symbols = append(i.symbols, SymbolInfo{
					Name:         fn.Name,
					Kind:         symbolKindForFunction(fn),
					Location:     fn.Location,
					Visibility:   fn.Visibility,
					IsExported:   fn.IsExported,
					ReceiverType: fn.ReceiverType,
					RawNode:      node,
				})
				currentFunction = fn.Name
			}
		}

		// Classes
		if classTypeSet[nodeType] {
			cls := i.extractClassInfo(node, src, lang)
			if cls != nil {
				i.classes = append(i.classes, *cls)
				i.symbols = append(i.symbols, SymbolInfo{
					Name:       cls.Name,
					Kind:       symbolKindForClass(cls),
					Location:   cls.Location,
					Visibility: cls.Visibility,
					IsExported: cls.IsExported,
					RawNode:    node,
				})
			}
		}

		// Imports
		if importTypeSet[nodeType] {
			imp := i.extractImportInfo(node, src, lang)
			if imp != nil {
				i.imports = append(i.imports, *imp)
			}
		}

		// Variables (top-level only for dead code detection)
		if varTypeSet[nodeType] {
			sym := i.extractVariableInfo(node, src, lang)
			if sym != nil {
				i.symbols = append(i.symbols, *sym)
			}
		}

		// Call expressions
		if isCallExpression(nodeType, lang) && currentFunction != "" {
			edge := i.extractCallEdge(node, src, lang, currentFunction)
			if edge != nil {
				i.callGraph = append(i.callGraph, *edge)
			}
		}

		return true
	})
}

// extractFunctionInfo extracts function information from an AST node.
func (i *TreeSitterInspector) extractFunctionInfo(node *sitter.Node, source []byte, lang Language) *FunctionInfo {
	fn := &FunctionInfo{
		Location: Location{
			File:      i.result.Path,
			StartLine: node.StartPoint().Row + 1,
			EndLine:   node.EndPoint().Row + 1,
			StartCol:  node.StartPoint().Column,
			EndCol:    node.EndPoint().Column,
		},
	}

	// Extract name
	if nameNode := node.ChildByFieldName("name"); nameNode != nil {
		fn.Name = GetNodeText(nameNode, source)
	}

	// For arrow functions assigned to variables
	if fn.Name == "" && node.Type() == "arrow_function" {
		fn.Name = getArrowFunctionNameFromParent(node, source)
	}

	if fn.Name == "" {
		return nil // Anonymous function
	}

	// Extract receiver type for methods
	fn.ReceiverType = extractMethodReceiverType(node, source, lang)

	// Determine visibility and export status
	fn.Visibility = getSymbolVisibility(fn.Name, lang)
	fn.IsExported = isSymbolExported(fn.Name, lang)

	// Extract signature
	body := node.ChildByFieldName("body")
	if body == nil {
		body = node.ChildByFieldName("block")
	}
	fn.Signature = extractFunctionSignature(node, source, lang, body)

	// Check for test functions, FFI, etc.
	fn.IsTestFunc = isTestFunction(fn.Name)
	fn.IsFFI = checkFFIExport(node, source, lang)

	return fn
}

// extractClassInfo extracts class information from an AST node.
func (i *TreeSitterInspector) extractClassInfo(node *sitter.Node, source []byte, lang Language) *ClassInfo {
	cls := &ClassInfo{
		Location: Location{
			File:      i.result.Path,
			StartLine: node.StartPoint().Row + 1,
			EndLine:   node.EndPoint().Row + 1,
			StartCol:  node.StartPoint().Column,
			EndCol:    node.EndPoint().Column,
		},
		Kind: classKindForNodeType(node.Type(), lang),
	}

	// Extract name
	if nameNode := node.ChildByFieldName("name"); nameNode != nil {
		cls.Name = GetNodeText(nameNode, source)
	}

	if cls.Name == "" {
		// Try type_identifier for some languages
		for j := range int(node.ChildCount()) {
			child := node.Child(j)
			if child.Type() == "type_identifier" || child.Type() == "identifier" {
				cls.Name = GetNodeText(child, source)
				break
			}
		}
	}

	if cls.Name == "" {
		return nil
	}

	cls.Visibility = getSymbolVisibility(cls.Name, lang)
	cls.IsExported = isSymbolExported(cls.Name, lang)

	// Extract inheritance info
	cls.Extends, cls.Implements = extractInheritance(node, source, lang)

	return cls
}

// extractImportInfo extracts import information from an AST node.
func (i *TreeSitterInspector) extractImportInfo(node *sitter.Node, source []byte, lang Language) *ImportInfo {
	imp := &ImportInfo{
		Location: Location{
			File:      i.result.Path,
			StartLine: node.StartPoint().Row + 1,
			EndLine:   node.EndPoint().Row + 1,
		},
	}

	switch lang {
	case LangGo:
		if pathNode := node.ChildByFieldName("path"); pathNode != nil {
			text := GetNodeText(pathNode, source)
			if len(text) > 2 {
				imp.Module = text[1 : len(text)-1] // Remove quotes
			}
		}
	case LangTypeScript, LangJavaScript, LangTSX:
		if sourceNode := node.ChildByFieldName("source"); sourceNode != nil {
			text := GetNodeText(sourceNode, source)
			if len(text) > 2 {
				imp.Module = text[1 : len(text)-1] // Remove quotes
			}
		}
		// Check for default import
		for j := range int(node.ChildCount()) {
			child := node.Child(j)
			if child.Type() == "identifier" {
				imp.IsDefault = true
				imp.Names = append(imp.Names, GetNodeText(child, source))
			}
		}
	case LangPython:
		if modNode := node.ChildByFieldName("module"); modNode != nil {
			imp.Module = GetNodeText(modNode, source)
		}
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			imp.Names = append(imp.Names, GetNodeText(nameNode, source))
		}
	case LangRust:
		if argNode := node.ChildByFieldName("argument"); argNode != nil {
			imp.Module = GetNodeText(argNode, source)
		}
	default:
		// Generic extraction
		if pathNode := node.ChildByFieldName("path"); pathNode != nil {
			imp.Module = GetNodeText(pathNode, source)
		} else if sourceNode := node.ChildByFieldName("source"); sourceNode != nil {
			imp.Module = GetNodeText(sourceNode, source)
		}
	}

	if imp.Module == "" {
		return nil
	}

	return imp
}

// extractVariableInfo extracts variable symbol information.
func (i *TreeSitterInspector) extractVariableInfo(node *sitter.Node, source []byte, lang Language) *SymbolInfo {
	sym := &SymbolInfo{
		Kind: SymbolVariable,
		Location: Location{
			File:      i.result.Path,
			StartLine: node.StartPoint().Row + 1,
			EndLine:   node.EndPoint().Row + 1,
		},
		RawNode: node,
	}

	// Extract name based on language
	switch lang {
	case LangGo:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			sym.Name = GetNodeText(nameNode, source)
		} else {
			// For short_var_declaration, look for identifier
			for j := range int(node.ChildCount()) {
				child := node.Child(j)
				if child.Type() == "identifier" {
					sym.Name = GetNodeText(child, source)
					break
				}
			}
		}
		if node.Type() == "const_declaration" {
			sym.Kind = SymbolConstant
		}
	case LangTypeScript, LangJavaScript, LangTSX:
		for j := range int(node.ChildCount()) {
			child := node.Child(j)
			if child.Type() == "variable_declarator" {
				if nameNode := child.ChildByFieldName("name"); nameNode != nil {
					sym.Name = GetNodeText(nameNode, source)
				}
				break
			}
		}
	case LangRust:
		if patternNode := node.ChildByFieldName("pattern"); patternNode != nil {
			sym.Name = GetNodeText(patternNode, source)
		}
		if node.Type() == "const_item" {
			sym.Kind = SymbolConstant
		}
	default:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			sym.Name = GetNodeText(nameNode, source)
		}
	}

	if sym.Name == "" {
		return nil
	}

	sym.Visibility = getSymbolVisibility(sym.Name, lang)
	sym.IsExported = isSymbolExported(sym.Name, lang)

	return sym
}

// extractCallEdge extracts a call edge from a call expression node.
func (i *TreeSitterInspector) extractCallEdge(node *sitter.Node, source []byte, lang Language, caller string) *CallEdge {
	edge := &CallEdge{
		CallerName: caller,
		Kind:       CallDirect,
		Location: Location{
			File:      i.result.Path,
			StartLine: node.StartPoint().Row + 1,
			EndLine:   node.EndPoint().Row + 1,
		},
	}

	fnNode := node.ChildByFieldName("function")
	if fnNode == nil {
		// Try first child
		if node.ChildCount() > 0 {
			fnNode = node.Child(0)
		}
	}

	if fnNode == nil {
		return nil
	}

	// Handle method calls vs direct calls
	fnType := fnNode.Type()
	switch fnType {
	case "member_expression", "field_expression", "selector_expression":
		edge.Kind = CallMethod
		// Extract callee name from property/field
		if propNode := fnNode.ChildByFieldName("property"); propNode != nil {
			edge.CalleeName = GetNodeText(propNode, source)
		} else if fieldNode := fnNode.ChildByFieldName("field"); fieldNode != nil {
			edge.CalleeName = GetNodeText(fieldNode, source)
		}
		// Extract receiver type from object/operand
		if objNode := fnNode.ChildByFieldName("object"); objNode != nil {
			edge.ReceiverType = GetNodeText(objNode, source)
		} else if opNode := fnNode.ChildByFieldName("operand"); opNode != nil {
			edge.ReceiverType = GetNodeText(opNode, source)
		}
	default:
		// Direct function calls (identifier, scoped_identifier, or other)
		edge.CalleeName = GetNodeText(fnNode, source)
	}

	// Handle Ruby-style calls
	if lang == LangRuby && (node.Type() == "call" || node.Type() == "method_call") {
		if methodNode := node.ChildByFieldName("method"); methodNode != nil {
			edge.CalleeName = GetNodeText(methodNode, source)
		}
		if receiverNode := node.ChildByFieldName("receiver"); receiverNode != nil {
			edge.ReceiverType = GetNodeText(receiverNode, source)
			edge.Kind = CallMethod
		}
	}

	if edge.CalleeName == "" {
		return nil
	}

	return edge
}

// Helper functions

func makeSet(items []string) map[string]bool {
	set := make(map[string]bool)
	for _, item := range items {
		set[item] = true
	}
	return set
}

func symbolKindForFunction(fn *FunctionInfo) SymbolKind {
	if fn.ReceiverType != "" {
		return SymbolMethod
	}
	return SymbolFunction
}

func symbolKindForClass(cls *ClassInfo) SymbolKind {
	switch cls.Kind {
	case "struct":
		return SymbolStruct
	default:
		return SymbolClass
	}
}

func getImportNodeTypes(lang Language) []string {
	switch lang {
	case LangGo:
		return []string{"import_declaration", "import_spec"}
	case LangRust:
		return []string{"use_declaration"}
	case LangPython:
		return []string{"import_statement", "import_from_statement"}
	case LangTypeScript, LangJavaScript, LangTSX:
		return []string{"import_statement", "import_declaration"}
	case LangJava:
		return []string{"import_declaration"}
	case LangCSharp:
		return []string{"using_directive"}
	case LangRuby:
		return []string{"call"} // require/require_relative
	case LangPHP:
		return []string{"use_declaration", "namespace_use_declaration"}
	default:
		return []string{"import_statement", "import_declaration"}
	}
}

func getVariableNodeTypes(lang Language) []string {
	switch lang {
	case LangGo:
		return []string{"var_declaration", "const_declaration", "short_var_declaration"}
	case LangRust:
		return []string{"let_declaration", "const_item", "static_item"}
	case LangPython:
		return []string{"assignment"}
	case LangTypeScript, LangJavaScript, LangTSX:
		return []string{"variable_declaration", "lexical_declaration"}
	case LangJava, LangCSharp:
		return []string{"local_variable_declaration", "field_declaration"}
	case LangC, LangCPP:
		return []string{"declaration", "init_declarator"}
	case LangRuby:
		return []string{"assignment"}
	case LangPHP:
		return []string{"simple_variable", "property_declaration"}
	default:
		return []string{"variable_declaration", "assignment"}
	}
}

func isCallExpression(nodeType string, lang Language) bool {
	switch nodeType {
	case "call_expression", "function_call", "invocation_expression":
		return true
	case "call", "method_call":
		return lang == LangRuby
	}
	return false
}

func getSymbolVisibility(name string, lang Language) Visibility {
	switch lang {
	case LangGo:
		if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
			return VisibilityPublic
		}
		return VisibilityPrivate
	case LangPython:
		if len(name) > 1 && name[0] == '_' && name[1] == '_' {
			return VisibilityPrivate
		}
		if len(name) > 0 && name[0] == '_' {
			return VisibilityInternal
		}
		return VisibilityPublic
	case LangRuby:
		if len(name) > 0 && name[0] == '_' {
			return VisibilityPrivate
		}
		return VisibilityPublic
	default:
		return VisibilityUnknown
	}
}

func isSymbolExported(name string, lang Language) bool {
	switch lang {
	case LangGo:
		return len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z'
	case LangPython:
		return len(name) == 0 || name[0] != '_'
	default:
		return false // Conservative default
	}
}

func classKindForNodeType(nodeType string, lang Language) string {
	switch nodeType {
	case "struct_item", "struct_specifier", "struct_declaration":
		return "struct"
	case "interface_declaration":
		return "interface"
	case "trait_item", "trait_declaration":
		return "trait"
	case "type_declaration", "type_spec", "type_alias":
		return "type"
	case "module":
		return "module"
	default:
		return "class"
	}
}

func extractInheritance(node *sitter.Node, source []byte, lang Language) (extends, implements []string) {
	switch lang {
	case LangJava, LangCSharp:
		if extendsNode := node.ChildByFieldName("superclass"); extendsNode != nil {
			extends = append(extends, GetNodeText(extendsNode, source))
		}
		if implNode := node.ChildByFieldName("interfaces"); implNode != nil {
			for j := range int(implNode.ChildCount()) {
				child := implNode.Child(j)
				if child.Type() == "type_identifier" {
					implements = append(implements, GetNodeText(child, source))
				}
			}
		}
	case LangTypeScript, LangJavaScript, LangTSX:
		// Look for heritage clause
		for j := range int(node.ChildCount()) {
			child := node.Child(j)
			if child.Type() == "class_heritage" || child.Type() == "extends_clause" {
				for k := range int(child.ChildCount()) {
					grandchild := child.Child(k)
					if grandchild.Type() == "identifier" || grandchild.Type() == "type_identifier" {
						extends = append(extends, GetNodeText(grandchild, source))
					}
				}
			}
		}
	case LangPython:
		if argsNode := node.ChildByFieldName("superclasses"); argsNode != nil {
			for j := range int(argsNode.ChildCount()) {
				child := argsNode.Child(j)
				if child.Type() == "identifier" {
					extends = append(extends, GetNodeText(child, source))
				}
			}
		}
	case LangRuby:
		if superNode := node.ChildByFieldName("superclass"); superNode != nil {
			extends = append(extends, GetNodeText(superNode, source))
		}
	}
	return
}

func extractMethodReceiverType(node *sitter.Node, source []byte, lang Language) string {
	if lang != LangGo {
		return ""
	}
	if receiver := node.ChildByFieldName("receiver"); receiver != nil {
		for j := range int(receiver.ChildCount()) {
			child := receiver.Child(j)
			if child.Type() == "parameter_declaration" {
				if typeNode := child.ChildByFieldName("type"); typeNode != nil {
					text := GetNodeText(typeNode, source)
					// Remove pointer prefix
					if len(text) > 0 && text[0] == '*' {
						return text[1:]
					}
					return text
				}
			}
		}
	}
	return ""
}

func getArrowFunctionNameFromParent(node *sitter.Node, source []byte) string {
	parent := node.Parent()
	if parent == nil {
		return ""
	}
	if parent.Type() == "variable_declarator" {
		if nameNode := parent.ChildByFieldName("name"); nameNode != nil {
			return GetNodeText(nameNode, source)
		}
	}
	return ""
}

func extractFunctionSignature(node *sitter.Node, source []byte, lang Language, body *sitter.Node) string {
	fullText := GetNodeText(node, source)
	if fullText == "" || body == nil {
		return normalizeSignature(fullText)
	}

	funcStart := node.StartByte()
	bodyStart := body.StartByte()
	if bodyStart <= funcStart {
		return normalizeSignature(fullText)
	}

	sigEnd := bodyStart - funcStart
	if sigEnd > uint32(len(fullText)) {
		sigEnd = uint32(len(fullText))
	}

	return normalizeSignature(fullText[:sigEnd])
}

func isTestFunction(name string) bool {
	if len(name) >= 4 {
		prefix := name[:4]
		if prefix == "Test" || prefix == "test" {
			return true
		}
	}
	if len(name) >= 9 && name[:9] == "Benchmark" {
		return true
	}
	if len(name) >= 7 && name[:7] == "Example" {
		return true
	}
	return false
}

func checkFFIExport(node *sitter.Node, source []byte, lang Language) bool {
	startByte := node.StartByte()
	if startByte == 0 {
		return false
	}

	searchStart := uint32(0)
	if startByte > 200 {
		searchStart = startByte - 200
	}

	precedingText := string(source[searchStart:startByte])

	switch lang {
	case LangGo:
		return contains(precedingText, "//export ") || contains(precedingText, "//go:linkname")
	case LangRust:
		return contains(precedingText, "#[no_mangle]") || contains(precedingText, "extern \"C\"")
	case LangC, LangCPP:
		nodeText := GetNodeText(node, source)
		return contains(nodeText, "__declspec(dllexport)") || contains(nodeText, "__attribute__((visibility")
	}
	return false
}

func contains(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// InspectFile is a convenience function that creates an inspector and extracts all data.
func InspectFile(path string) (Inspector, error) {
	factory := NewTreeSitterInspectorFactory()
	return factory.FromFile(path)
}

// InspectFileWithContext is like InspectFile but respects context cancellation.
func InspectFileWithContext(ctx context.Context, path string) (Inspector, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	default:
		return InspectFile(path)
	}
}
