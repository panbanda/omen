package analyzer

import (
	"time"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// CohesionAnalyzer computes CK (Chidamber-Kemerer) metrics for OO code.
type CohesionAnalyzer struct {
	parser       *parser.Parser
	complexity   *ComplexityAnalyzer
	skipTestFile bool
	maxFileSize  int64
}

// CohesionOption is a functional option for configuring CohesionAnalyzer.
type CohesionOption func(*CohesionAnalyzer)

// WithCohesionSkipTestFiles enables or disables skipping test files.
func WithCohesionSkipTestFiles(skip bool) CohesionOption {
	return func(a *CohesionAnalyzer) {
		a.skipTestFile = skip
	}
}

// WithCohesionMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithCohesionMaxFileSize(maxSize int64) CohesionOption {
	return func(a *CohesionAnalyzer) {
		a.maxFileSize = maxSize
	}
}

// NewCohesionAnalyzer creates a new CK metrics analyzer.
func NewCohesionAnalyzer(opts ...CohesionOption) *CohesionAnalyzer {
	a := &CohesionAnalyzer{
		parser:       parser.New(),
		complexity:   NewComplexityAnalyzer(),
		skipTestFile: true,
		maxFileSize:  0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// isOOLanguage returns true if the language supports traditional OO classes.
func isOOLanguage(lang parser.Language) bool {
	switch lang {
	case parser.LangJava, parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX,
		parser.LangPython, parser.LangCSharp, parser.LangCPP, parser.LangRuby, parser.LangPHP:
		return true
	default:
		return false
	}
}

// isTestFilePath checks if a file path suggests a test file.
// Uses the shared IsTestFile function.
var isTestFilePath = IsTestFile

// AnalyzeProject computes CK metrics for all OO classes in the project.
func (a *CohesionAnalyzer) AnalyzeProject(files []string) (*models.CohesionAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress computes CK metrics with progress callback.
func (a *CohesionAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress fileproc.ProgressFunc) (*models.CohesionAnalysis, error) {
	analysis := &models.CohesionAnalysis{
		GeneratedAt: time.Now().UTC(),
		Classes:     make([]models.ClassMetrics, 0),
	}

	// First pass: Build inheritance tree across all files
	inheritanceTree := a.buildInheritanceTree(files)

	// Second pass: Process files in parallel to extract CK metrics
	results := fileproc.MapFilesWithSizeLimit(files, a.maxFileSize, func(psr *parser.Parser, path string) ([]models.ClassMetrics, error) {
		if a.skipTestFile && isTestFilePath(path) {
			return nil, nil
		}
		return a.analyzeFile(psr, path, inheritanceTree)
	}, onProgress, nil)

	// Collect results
	for _, fileClasses := range results {
		analysis.Classes = append(analysis.Classes, fileClasses...)
	}

	// Sort by LCOM (least cohesive first)
	analysis.SortByLCOM()
	analysis.CalculateSummary()

	return analysis, nil
}

// inheritanceInfo holds inheritance relationship data for a class.
type inheritanceInfo struct {
	className string
	parents   []string // Direct parent classes (supports multiple inheritance)
	file      string
}

// inheritanceTree holds the project-wide inheritance graph.
type inheritanceTree struct {
	// classToParents maps class name to its direct parent class names
	classToParents map[string][]string
	// classToChildren maps class name to its direct child class names
	classToChildren map[string][]string
	// allClasses tracks all known class names
	allClasses map[string]bool
}

// buildInheritanceTree scans all files to build the inheritance graph.
func (a *CohesionAnalyzer) buildInheritanceTree(files []string) *inheritanceTree {
	tree := &inheritanceTree{
		classToParents:  make(map[string][]string),
		classToChildren: make(map[string][]string),
		allClasses:      make(map[string]bool),
	}

	// Extract inheritance info from all files in parallel
	results := fileproc.MapFiles(files, func(psr *parser.Parser, path string) ([]inheritanceInfo, error) {
		if a.skipTestFile && isTestFilePath(path) {
			return nil, nil
		}
		return extractInheritanceInfo(psr, path)
	})

	// Track children relationships to avoid duplicates
	childrenSet := make(map[string]map[string]bool)

	// Build the tree from results
	for _, fileInfo := range results {
		for _, info := range fileInfo {
			tree.allClasses[info.className] = true
			tree.classToParents[info.className] = info.parents

			// Build reverse mapping (parent -> children) with deduplication
			for _, parent := range info.parents {
				if childrenSet[parent] == nil {
					childrenSet[parent] = make(map[string]bool)
				}
				if !childrenSet[parent][info.className] {
					childrenSet[parent][info.className] = true
					tree.classToChildren[parent] = append(tree.classToChildren[parent], info.className)
				}
			}
		}
	}

	return tree
}

// getDIT calculates Depth of Inheritance Tree for a class.
// DIT is the maximum length from the class to the root of the inheritance tree.
func (tree *inheritanceTree) getDIT(className string) int {
	return tree.calculateDepth(className, make(map[string]bool))
}

// calculateDepth recursively calculates inheritance depth, handling cycles.
func (tree *inheritanceTree) calculateDepth(className string, visited map[string]bool) int {
	if visited[className] {
		return 0 // Cycle detected, stop recursion
	}
	visited[className] = true

	parents := tree.classToParents[className]
	if len(parents) == 0 {
		return 0 // Root class (no parents)
	}

	maxParentDepth := 0
	for _, parent := range parents {
		depth := tree.calculateDepth(parent, visited)
		if depth > maxParentDepth {
			maxParentDepth = depth
		}
	}

	return maxParentDepth + 1
}

// getNOC returns Number of Children for a class.
// NOC is the count of immediate subclasses.
func (tree *inheritanceTree) getNOC(className string) int {
	return len(tree.classToChildren[className])
}

// extractInheritanceInfo extracts class inheritance from a file.
func extractInheritanceInfo(psr *parser.Parser, path string) ([]inheritanceInfo, error) {
	result, err := psr.ParseFile(path)
	if err != nil {
		return nil, err
	}

	if !isOOLanguage(result.Language) {
		return nil, nil
	}

	var infos []inheritanceInfo
	root := result.Tree.RootNode()

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		if isClassNode(node.Type(), result.Language) {
			info := inheritanceInfo{file: path}

			// Get class name
			if nameNode := node.ChildByFieldName("name"); nameNode != nil {
				info.className = parser.GetNodeText(nameNode, source)
			}

			if info.className == "" {
				return true
			}

			// Extract parent classes based on language
			info.parents = extractParentClasses(node, source, result.Language)
			infos = append(infos, info)
		}
		return true
	})

	return infos, nil
}

// extractParentClasses extracts parent class names from a class declaration.
func extractParentClasses(classNode *sitter.Node, source []byte, lang parser.Language) []string {
	var parents []string

	switch lang {
	case parser.LangJava:
		// Java: class Foo extends Bar implements Baz, Qux
		if superclass := classNode.ChildByFieldName("superclass"); superclass != nil {
			parents = append(parents, parser.GetNodeText(superclass, source))
		}
		if interfaces := classNode.ChildByFieldName("interfaces"); interfaces != nil {
			parents = append(parents, extractTypeList(interfaces, source)...)
		}

	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		// TS/JS: class Foo extends Bar implements Baz
		// Look for heritage clauses
		for i := 0; i < int(classNode.ChildCount()); i++ {
			child := classNode.Child(i)
			if child.Type() == "class_heritage" || child.Type() == "extends_clause" {
				parents = append(parents, extractHeritageTypes(child, source)...)
			}
		}

	case parser.LangPython:
		// Python: class Foo(Bar, Baz):
		if argList := classNode.ChildByFieldName("superclasses"); argList != nil {
			parents = append(parents, extractArgumentList(argList, source)...)
		}
		// Also check argument_list for older tree-sitter versions
		for i := 0; i < int(classNode.ChildCount()); i++ {
			child := classNode.Child(i)
			if child.Type() == "argument_list" {
				parents = append(parents, extractArgumentList(child, source)...)
			}
		}

	case parser.LangCSharp:
		// C#: class Foo : Bar, IBaz
		if baseList := classNode.ChildByFieldName("bases"); baseList != nil {
			parents = append(parents, extractTypeList(baseList, source)...)
		}

	case parser.LangCPP:
		// C++: class Foo : public Bar, private Baz
		for i := 0; i < int(classNode.ChildCount()); i++ {
			child := classNode.Child(i)
			if child.Type() == "base_class_clause" {
				parents = append(parents, extractBaseClasses(child, source)...)
			}
		}

	case parser.LangRuby:
		// Ruby: class Foo < Bar
		if superclass := classNode.ChildByFieldName("superclass"); superclass != nil {
			parents = append(parents, parser.GetNodeText(superclass, source))
		}

	case parser.LangPHP:
		// PHP: class Foo extends Bar implements Baz
		if extends := classNode.ChildByFieldName("extends"); extends != nil {
			parents = append(parents, parser.GetNodeText(extends, source))
		}
		if implements := classNode.ChildByFieldName("implements"); implements != nil {
			parents = append(parents, extractTypeList(implements, source)...)
		}
	}

	// Filter out empty strings and clean up
	var cleaned []string
	for _, p := range parents {
		p = cleanTypeName(p)
		if p != "" && !isPrimitiveType(p) {
			cleaned = append(cleaned, p)
		}
	}

	return cleaned
}

// extractTypeList extracts type names from a type list node.
func extractTypeList(node *sitter.Node, source []byte) []string {
	var types []string
	for i := 0; i < int(node.ChildCount()); i++ {
		child := node.Child(i)
		if child.Type() == "type_identifier" || child.Type() == "identifier" ||
			child.Type() == "simple_type" || child.Type() == "named_type" ||
			child.Type() == "generic_type" || child.Type() == "type_list" {
			name := cleanTypeName(parser.GetNodeText(child, source))
			if name != "" && name != "," {
				types = append(types, name)
			}
		}
	}
	return types
}

// extractHeritageTypes extracts types from TS/JS heritage clauses.
func extractHeritageTypes(node *sitter.Node, source []byte) []string {
	var types []string
	parser.Walk(node, source, func(n *sitter.Node, s []byte) bool {
		if n.Type() == "identifier" || n.Type() == "type_identifier" {
			name := parser.GetNodeText(n, s)
			if name != "" && name != "extends" && name != "implements" {
				types = append(types, name)
			}
		}
		return true
	})
	return types
}

// extractArgumentList extracts identifiers from Python argument list.
func extractArgumentList(node *sitter.Node, source []byte) []string {
	var args []string
	for i := 0; i < int(node.ChildCount()); i++ {
		child := node.Child(i)
		if child.Type() == "identifier" || child.Type() == "attribute" {
			name := parser.GetNodeText(child, source)
			if name != "" && name != "," && name != "(" && name != ")" {
				args = append(args, name)
			}
		}
	}
	return args
}

// extractBaseClasses extracts base classes from C++ base_class_clause.
func extractBaseClasses(node *sitter.Node, source []byte) []string {
	var bases []string
	parser.Walk(node, source, func(n *sitter.Node, s []byte) bool {
		if n.Type() == "type_identifier" || n.Type() == "identifier" {
			name := parser.GetNodeText(n, s)
			// Filter out access specifiers
			if name != "" && name != "public" && name != "private" && name != "protected" {
				bases = append(bases, name)
			}
		}
		return true
	})
	return bases
}

// cleanTypeName removes generic parameters and cleans up type names.
func cleanTypeName(name string) string {
	// Remove generic parameters: List<String> -> List
	for i, c := range name {
		if c == '<' || c == '[' {
			name = name[:i]
			break
		}
	}
	// Trim whitespace
	for len(name) > 0 && (name[0] == ' ' || name[0] == '\t') {
		name = name[1:]
	}
	for len(name) > 0 && (name[len(name)-1] == ' ' || name[len(name)-1] == '\t') {
		name = name[:len(name)-1]
	}
	return name
}

// analyzeFile extracts CK metrics from a single file.
func (a *CohesionAnalyzer) analyzeFile(psr *parser.Parser, path string, tree *inheritanceTree) ([]models.ClassMetrics, error) {
	result, err := psr.ParseFile(path)
	if err != nil {
		return nil, err
	}

	// Only process OO languages
	if !isOOLanguage(result.Language) {
		return nil, nil
	}

	var classes []models.ClassMetrics

	// Find all class definitions
	classNodes := parser.GetClasses(result)

	for _, cls := range classNodes {
		if cls.Name == "" {
			continue
		}

		metrics := models.ClassMetrics{
			Path:      path,
			ClassName: cls.Name,
			Language:  string(result.Language),
			StartLine: int(cls.StartLine),
			EndLine:   int(cls.EndLine),
			LOC:       int(cls.EndLine - cls.StartLine + 1),
		}

		// Find the class node in the AST for deeper analysis
		classNode := findClassNode(result, cls.Name)
		if classNode != nil {
			a.extractClassMetrics(classNode, result, &metrics, tree)
		}

		classes = append(classes, metrics)
	}

	return classes, nil
}

// findClassNode finds the AST node for a class by name.
func findClassNode(result *parser.ParseResult, className string) *sitter.Node {
	var found *sitter.Node
	root := result.Tree.RootNode()

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		if isClassNode(node.Type(), result.Language) {
			if nameNode := node.ChildByFieldName("name"); nameNode != nil {
				if parser.GetNodeText(nameNode, source) == className {
					found = node
					return false
				}
			}
		}
		return true
	})

	return found
}

// isClassNode checks if a node type represents a class.
func isClassNode(nodeType string, lang parser.Language) bool {
	switch lang {
	case parser.LangJava:
		return nodeType == "class_declaration" || nodeType == "interface_declaration"
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return nodeType == "class_declaration" || nodeType == "class"
	case parser.LangPython:
		return nodeType == "class_definition"
	case parser.LangCSharp:
		return nodeType == "class_declaration" || nodeType == "interface_declaration"
	case parser.LangCPP:
		return nodeType == "class_specifier" || nodeType == "struct_specifier"
	case parser.LangRuby:
		return nodeType == "class" || nodeType == "module"
	case parser.LangPHP:
		return nodeType == "class_declaration" || nodeType == "interface_declaration"
	default:
		return false
	}
}

// extractClassMetrics extracts CK metrics from a class AST node.
func (a *CohesionAnalyzer) extractClassMetrics(classNode *sitter.Node, result *parser.ParseResult, metrics *models.ClassMetrics, tree *inheritanceTree) {
	// Extract methods and their complexity
	methods := extractMethodsFromClass(classNode, result)
	metrics.NOM = len(methods)
	metrics.Methods = make([]string, 0, len(methods))

	// WMC = sum of cyclomatic complexity of all methods
	for _, m := range methods {
		metrics.WMC += m.complexity
		metrics.Methods = append(metrics.Methods, m.name)
	}

	// Extract fields
	fields := extractFieldsFromClass(classNode, result)
	metrics.NOF = len(fields)
	metrics.Fields = fields

	// RFC = number of methods that can be called
	// RFC = local methods + methods called from within the class
	calledMethods := extractCalledMethods(classNode, result)
	metrics.RFC = len(methods) + len(calledMethods)

	// CBO = coupling between objects (number of distinct classes referenced)
	coupledClasses := extractCoupledClasses(classNode, result)
	metrics.CBO = len(coupledClasses)
	metrics.CoupledClasses = coupledClasses

	// LCOM = Lack of Cohesion in Methods (using LCOM4 variant)
	// LCOM4 counts connected components in method-field usage graph
	metrics.LCOM = calculateLCOM4(methods, fields)

	// DIT = Depth of Inheritance Tree (max path length to root)
	// NOC = Number of Children (count of immediate subclasses)
	if tree != nil {
		metrics.DIT = tree.getDIT(metrics.ClassName)
		metrics.NOC = tree.getNOC(metrics.ClassName)
	}
}

// methodInfo holds method data for LCOM calculation.
type methodInfo struct {
	name       string
	complexity int
	usedFields map[string]bool
}

// extractMethodsFromClass extracts methods from a class node.
func extractMethodsFromClass(classNode *sitter.Node, result *parser.ParseResult) []methodInfo {
	var methods []methodInfo

	methodTypes := getMethodNodeTypes(result.Language)

	parser.Walk(classNode, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, mt := range methodTypes {
			if node.Type() == mt {
				m := methodInfo{
					usedFields: make(map[string]bool),
				}

				// Get method name
				if nameNode := node.ChildByFieldName("name"); nameNode != nil {
					m.name = parser.GetNodeText(nameNode, source)
				}

				// Calculate cyclomatic complexity
				m.complexity = calculateNodeComplexity(node, result.Language)

				// Find fields used by this method
				m.usedFields = findFieldsUsedByMethod(node, result)

				if m.name != "" {
					methods = append(methods, m)
				}
				return false // Don't descend into nested methods
			}
		}
		return true
	})

	return methods
}

// getMethodNodeTypes returns AST node types for methods.
func getMethodNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangJava:
		return []string{"method_declaration", "constructor_declaration"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"method_definition", "public_field_definition"}
	case parser.LangPython:
		return []string{"function_definition"}
	case parser.LangCSharp:
		return []string{"method_declaration", "constructor_declaration"}
	case parser.LangCPP:
		return []string{"function_definition", "function_declarator"}
	case parser.LangRuby:
		return []string{"method", "singleton_method"}
	case parser.LangPHP:
		return []string{"method_declaration"}
	default:
		return nil
	}
}

// extractFieldsFromClass extracts field/attribute names from a class.
func extractFieldsFromClass(classNode *sitter.Node, result *parser.ParseResult) []string {
	var fields []string
	fieldTypes := getFieldNodeTypes(result.Language)

	parser.Walk(classNode, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, ft := range fieldTypes {
			if node.Type() == ft {
				name := extractFieldName(node, source, result.Language)
				if name != "" {
					fields = append(fields, name)
				}
			}
		}
		return true
	})

	return fields
}

// getFieldNodeTypes returns AST node types for fields/attributes.
func getFieldNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangJava:
		return []string{"field_declaration"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"public_field_definition", "field_definition"}
	case parser.LangPython:
		return []string{"assignment"} // self.field = value
	case parser.LangCSharp:
		return []string{"field_declaration", "property_declaration"}
	case parser.LangCPP:
		return []string{"field_declaration"}
	case parser.LangRuby:
		return []string{"instance_variable"}
	case parser.LangPHP:
		return []string{"property_declaration"}
	default:
		return nil
	}
}

// extractFieldName extracts the field name from a field node.
func extractFieldName(node *sitter.Node, source []byte, lang parser.Language) string {
	switch lang {
	case parser.LangPython:
		// Look for self.field = ... pattern
		if node.Type() == "assignment" {
			left := node.ChildByFieldName("left")
			if left != nil && left.Type() == "attribute" {
				obj := left.ChildByFieldName("object")
				attr := left.ChildByFieldName("attribute")
				if obj != nil && attr != nil {
					if parser.GetNodeText(obj, source) == "self" {
						return parser.GetNodeText(attr, source)
					}
				}
			}
		}
	case parser.LangRuby:
		// @field
		return parser.GetNodeText(node, source)
	default:
		// Look for declarator/name
		if nameNode := node.ChildByFieldName("declarator"); nameNode != nil {
			// Java/C++ declarators might have nested structure
			if innerName := nameNode.ChildByFieldName("name"); innerName != nil {
				return parser.GetNodeText(innerName, source)
			}
			return parser.GetNodeText(nameNode, source)
		}
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			return parser.GetNodeText(nameNode, source)
		}
	}
	return ""
}

// findFieldsUsedByMethod finds fields accessed within a method.
func findFieldsUsedByMethod(methodNode *sitter.Node, result *parser.ParseResult) map[string]bool {
	fields := make(map[string]bool)

	parser.Walk(methodNode, result.Source, func(node *sitter.Node, source []byte) bool {
		switch result.Language {
		case parser.LangPython:
			// self.field
			if node.Type() == "attribute" {
				obj := node.ChildByFieldName("object")
				attr := node.ChildByFieldName("attribute")
				if obj != nil && attr != nil {
					if parser.GetNodeText(obj, source) == "self" {
						fields[parser.GetNodeText(attr, source)] = true
					}
				}
			}
		case parser.LangRuby:
			// @field
			if node.Type() == "instance_variable" {
				fields[parser.GetNodeText(node, source)] = true
			}
		case parser.LangJava, parser.LangCSharp, parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
			// this.field or just field (requires context)
			if node.Type() == "member_expression" || node.Type() == "member_access_expression" {
				obj := node.ChildByFieldName("object")
				prop := node.ChildByFieldName("property")
				if obj != nil && prop != nil {
					if parser.GetNodeText(obj, source) == "this" {
						fields[parser.GetNodeText(prop, source)] = true
					}
				}
			}
		}
		return true
	})

	return fields
}

// extractCalledMethods finds methods called within a class.
func extractCalledMethods(classNode *sitter.Node, result *parser.ParseResult) []string {
	called := make(map[string]bool)

	parser.Walk(classNode, result.Source, func(node *sitter.Node, source []byte) bool {
		if node.Type() == "call_expression" || node.Type() == "method_call" ||
			node.Type() == "invocation_expression" {
			if fnNode := node.ChildByFieldName("function"); fnNode != nil {
				called[parser.GetNodeText(fnNode, source)] = true
			}
			if nameNode := node.ChildByFieldName("name"); nameNode != nil {
				called[parser.GetNodeText(nameNode, source)] = true
			}
		}
		return true
	})

	var calledNames []string
	for name := range called {
		calledNames = append(calledNames, name)
	}
	return calledNames
}

// extractCoupledClasses finds classes referenced by this class.
func extractCoupledClasses(classNode *sitter.Node, result *parser.ParseResult) []string {
	coupled := make(map[string]bool)

	typeNodeTypes := []string{
		"type_identifier", "class_type", "simple_type",
		"named_type", "type_name", "identifier",
	}

	parser.Walk(classNode, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, tt := range typeNodeTypes {
			if node.Type() == tt {
				name := parser.GetNodeText(node, source)
				// Filter out primitives and common types
				if !isPrimitiveType(name) && len(name) > 1 {
					coupled[name] = true
				}
			}
		}
		return true
	})

	var classes []string
	for name := range coupled {
		classes = append(classes, name)
	}
	return classes
}

// primitiveTypes is a pre-allocated set of primitive type names.
var primitiveTypes = map[string]bool{
	"int": true, "int8": true, "int16": true, "int32": true, "int64": true,
	"uint": true, "uint8": true, "uint16": true, "uint32": true, "uint64": true,
	"float": true, "float32": true, "float64": true, "double": true,
	"bool": true, "boolean": true, "Boolean": true,
	"string": true, "String": true, "str": true,
	"void": true, "None": true, "null": true, "nil": true,
	"byte": true, "char": true, "short": true, "long": true,
	"any": true, "object": true, "Object": true,
	"number": true, "Number": true,
	"true": true, "false": true,
	"self": true, "this": true, "super": true,
}

// isPrimitiveType checks if a type name is a primitive.
func isPrimitiveType(name string) bool {
	return primitiveTypes[name]
}

// calculateNodeComplexity calculates cyclomatic complexity for a node.
func calculateNodeComplexity(node *sitter.Node, lang parser.Language) int {
	complexity := 1 // Base complexity

	decisionTypes := []string{
		"if_statement", "if_expression", "if",
		"for_statement", "for_expression", "for",
		"while_statement", "while_expression", "while",
		"switch_statement", "match_expression",
		"case_clause", "case_statement",
		"catch_clause", "except_clause",
		"conditional_expression", "ternary_expression",
		"&&", "||", "and", "or",
	}

	parser.Walk(node, nil, func(n *sitter.Node, source []byte) bool {
		for _, dt := range decisionTypes {
			if n.Type() == dt {
				complexity++
				break
			}
		}
		return true
	})

	return complexity
}

// calculateLCOM4 calculates LCOM4 (number of connected components).
// Methods sharing fields are in the same component.
func calculateLCOM4(methods []methodInfo, fields []string) int {
	if len(methods) == 0 {
		return 0
	}
	if len(fields) == 0 {
		// No fields means methods don't share state
		return len(methods)
	}

	// Build adjacency list: methods are connected if they share a field
	n := len(methods)
	adj := make([][]int, n)
	for i := range adj {
		adj[i] = make([]int, 0)
	}

	for i := 0; i < n; i++ {
		for j := i + 1; j < n; j++ {
			// Check if methods i and j share any field
			for field := range methods[i].usedFields {
				if methods[j].usedFields[field] {
					adj[i] = append(adj[i], j)
					adj[j] = append(adj[j], i)
					break
				}
			}
		}
	}

	// Count connected components using DFS
	visited := make([]bool, n)
	components := 0

	var dfs func(int)
	dfs = func(v int) {
		visited[v] = true
		for _, u := range adj[v] {
			if !visited[u] {
				dfs(u)
			}
		}
	}

	for i := 0; i < n; i++ {
		if !visited[i] {
			dfs(i)
			components++
		}
	}

	return components
}

// Close releases resources.
func (a *CohesionAnalyzer) Close() {
	if a.parser != nil {
		a.parser.Close()
	}
	if a.complexity != nil {
		a.complexity.Close()
	}
}
