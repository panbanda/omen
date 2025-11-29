package parser

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	sitter "github.com/smacker/go-tree-sitter"
	"github.com/smacker/go-tree-sitter/bash"
	"github.com/smacker/go-tree-sitter/c"
	"github.com/smacker/go-tree-sitter/cpp"
	"github.com/smacker/go-tree-sitter/csharp"
	"github.com/smacker/go-tree-sitter/golang"
	"github.com/smacker/go-tree-sitter/java"
	"github.com/smacker/go-tree-sitter/javascript"
	"github.com/smacker/go-tree-sitter/php"
	"github.com/smacker/go-tree-sitter/python"
	"github.com/smacker/go-tree-sitter/ruby"
	"github.com/smacker/go-tree-sitter/rust"
	"github.com/smacker/go-tree-sitter/typescript/tsx"
	"github.com/smacker/go-tree-sitter/typescript/typescript"
)

// Language represents a supported programming language.
type Language string

const (
	LangGo         Language = "go"
	LangRust       Language = "rust"
	LangPython     Language = "python"
	LangTypeScript Language = "typescript"
	LangJavaScript Language = "javascript"
	LangTSX        Language = "tsx"
	LangJava       Language = "java"
	LangC          Language = "c"
	LangCPP        Language = "cpp"
	LangCSharp     Language = "csharp"
	LangRuby       Language = "ruby"
	LangPHP        Language = "php"
	LangBash       Language = "bash"
	LangUnknown    Language = "unknown"
)

// Parser wraps tree-sitter for multi-language parsing.
type Parser struct {
	parser *sitter.Parser
}

// ParseResult contains the parsed AST and metadata.
type ParseResult struct {
	Tree     *sitter.Tree
	Language Language
	Source   []byte
	Path     string
}

// New creates a new parser instance.
func New() *Parser {
	return &Parser{
		parser: sitter.NewParser(),
	}
}

// ParseFile parses a source file and returns the AST.
func (p *Parser) ParseFile(path string) (*ParseResult, error) {
	source, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("failed to read file: %w", err)
	}

	lang := DetectLanguage(path)
	if lang == LangUnknown {
		return nil, fmt.Errorf("unsupported language for file: %s", path)
	}

	return p.Parse(source, lang, path)
}

// Parse parses source code with a specified language.
func (p *Parser) Parse(source []byte, lang Language, path string) (*ParseResult, error) {
	tsLang, err := GetTreeSitterLanguage(lang)
	if err != nil {
		return nil, err
	}

	p.parser.SetLanguage(tsLang)
	tree, err := p.parser.ParseCtx(context.Background(), nil, source)
	if err != nil {
		return nil, fmt.Errorf("failed to parse: %w", err)
	}

	return &ParseResult{
		Tree:     tree,
		Language: lang,
		Source:   source,
		Path:     path,
	}, nil
}

// GetTreeSitterLanguage returns the tree-sitter language for a Language enum.
func GetTreeSitterLanguage(lang Language) (*sitter.Language, error) {
	switch lang {
	case LangGo:
		return golang.GetLanguage(), nil
	case LangRust:
		return rust.GetLanguage(), nil
	case LangPython:
		return python.GetLanguage(), nil
	case LangTypeScript:
		return typescript.GetLanguage(), nil
	case LangTSX:
		return tsx.GetLanguage(), nil
	case LangJavaScript:
		return javascript.GetLanguage(), nil
	case LangJava:
		return java.GetLanguage(), nil
	case LangC:
		return c.GetLanguage(), nil
	case LangCPP:
		return cpp.GetLanguage(), nil
	case LangCSharp:
		return csharp.GetLanguage(), nil
	case LangRuby:
		return ruby.GetLanguage(), nil
	case LangPHP:
		return php.GetLanguage(), nil
	case LangBash:
		return bash.GetLanguage(), nil
	default:
		return nil, fmt.Errorf("unsupported language: %s", lang)
	}
}

// DetectLanguage determines the language from a file path.
func DetectLanguage(path string) Language {
	ext := strings.ToLower(filepath.Ext(path))
	base := strings.ToLower(filepath.Base(path))

	// Check special filenames first
	switch base {
	case "dockerfile":
		return LangBash // Use bash for Dockerfile
	}

	switch ext {
	case ".go":
		return LangGo
	case ".rs":
		return LangRust
	case ".py", ".pyw", ".pyi":
		return LangPython
	case ".ts":
		return LangTypeScript
	case ".tsx":
		return LangTSX
	case ".js", ".mjs", ".cjs":
		return LangJavaScript
	case ".jsx":
		return LangTSX // Use TSX parser for JSX
	case ".java":
		return LangJava
	case ".c", ".h":
		return LangC
	case ".cpp", ".cc", ".cxx", ".hpp", ".hxx":
		return LangCPP
	case ".cs":
		return LangCSharp
	case ".rb":
		return LangRuby
	case ".php":
		return LangPHP
	case ".sh", ".bash":
		return LangBash
	default:
		return LangUnknown
	}
}

// Close releases parser resources.
func (p *Parser) Close() {
	p.parser.Close()
}

// NodeVisitor is a function that visits AST nodes.
type NodeVisitor func(node *sitter.Node, source []byte) bool

// TypedNodeVisitor visits AST nodes with pre-cached node type to avoid CGO overhead.
type TypedNodeVisitor func(node *sitter.Node, nodeType string, source []byte) bool

// Walk traverses the AST calling visitor for each node.
func Walk(node *sitter.Node, source []byte, visitor NodeVisitor) {
	if node == nil {
		return
	}

	if !visitor(node, source) {
		return
	}

	for i := range int(node.ChildCount()) {
		Walk(node.Child(i), source, visitor)
	}
}

// WalkTyped traverses the AST with cached node types to reduce CGO overhead.
// Use this when you need to check node types frequently.
func WalkTyped(node *sitter.Node, source []byte, visitor TypedNodeVisitor) {
	if node == nil {
		return
	}

	nodeType := node.Type() // Cache the type once per node
	if !visitor(node, nodeType, source) {
		return
	}

	for i := range int(node.ChildCount()) {
		WalkTyped(node.Child(i), source, visitor)
	}
}

// FindNodes returns all nodes matching a predicate.
func FindNodes(root *sitter.Node, source []byte, predicate func(*sitter.Node) bool) []*sitter.Node {
	var results []*sitter.Node
	Walk(root, source, func(node *sitter.Node, source []byte) bool {
		if predicate(node) {
			results = append(results, node)
		}
		return true
	})
	return results
}

// FindNodesByType returns all nodes of a specific type.
func FindNodesByType(root *sitter.Node, source []byte, nodeType string) []*sitter.Node {
	return FindNodes(root, source, func(n *sitter.Node) bool {
		return n.Type() == nodeType
	})
}

// GetNodeText extracts the source text for a node.
// Returns empty string if node is nil or byte offsets are out of bounds.
func GetNodeText(node *sitter.Node, source []byte) string {
	if node == nil {
		return ""
	}
	start := node.StartByte()
	end := node.EndByte()
	if start > end || end > uint32(len(source)) {
		return ""
	}
	return string(source[start:end])
}

// FunctionNode represents a parsed function.
type FunctionNode struct {
	Name       string
	StartLine  uint32
	EndLine    uint32
	Parameters []string
	Body       *sitter.Node
}

// GetFunctions extracts all function definitions from parsed code.
func GetFunctions(result *ParseResult) []FunctionNode {
	var functions []FunctionNode
	root := result.Tree.RootNode()

	// Function node types vary by language
	funcTypes := getFunctionNodeTypes(result.Language)

	Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, ft := range funcTypes {
			if node.Type() == ft {
				fn := extractFunction(node, source, result.Language)
				if fn != nil {
					functions = append(functions, *fn)
				}
				break
			}
		}
		return true
	})

	return functions
}

// getFunctionNodeTypes returns the AST node types for functions in each language.
func getFunctionNodeTypes(lang Language) []string {
	switch lang {
	case LangGo:
		return []string{"function_declaration", "method_declaration"}
	case LangRust:
		return []string{"function_item"}
	case LangPython:
		return []string{"function_definition"}
	case LangTypeScript, LangJavaScript, LangTSX:
		return []string{"function_declaration", "function", "arrow_function", "method_definition"}
	case LangJava:
		return []string{"method_declaration", "constructor_declaration"}
	case LangC, LangCPP:
		return []string{"function_definition"}
	case LangCSharp:
		return []string{"method_declaration", "constructor_declaration"}
	case LangRuby:
		return []string{"method", "singleton_method"}
	case LangPHP:
		return []string{"function_definition", "method_declaration"}
	default:
		return nil
	}
}

// extractFunction extracts function details from an AST node.
func extractFunction(node *sitter.Node, source []byte, lang Language) *FunctionNode {
	fn := &FunctionNode{
		StartLine: node.StartPoint().Row + 1,
		EndLine:   node.EndPoint().Row + 1,
	}

	// Extract function name based on language
	switch lang {
	case LangGo:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	case LangRust:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	case LangPython:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	case LangTypeScript, LangJavaScript, LangTSX:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	case LangJava, LangCSharp:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	case LangC, LangCPP:
		// C/C++ function names are in declarator
		if declNode := node.ChildByFieldName("declarator"); declNode != nil {
			if nameNode := declNode.ChildByFieldName("declarator"); nameNode != nil {
				fn.Name = GetNodeText(nameNode, source)
			}
		}
	case LangRuby:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	case LangPHP:
		if nameNode := node.ChildByFieldName("name"); nameNode != nil {
			fn.Name = GetNodeText(nameNode, source)
		}
	}

	// Get body node - field names vary by language
	fn.Body = node.ChildByFieldName("body")
	if fn.Body == nil {
		fn.Body = node.ChildByFieldName("block")
	}
	if fn.Body == nil {
		// Ruby uses body_statement for method bodies
		fn.Body = node.ChildByFieldName("body_statement")
	}

	return fn
}

// ClassNode represents a parsed class/struct.
type ClassNode struct {
	Name      string
	StartLine uint32
	EndLine   uint32
	Methods   []FunctionNode
}

// GetClasses extracts all class definitions from parsed code.
func GetClasses(result *ParseResult) []ClassNode {
	var classes []ClassNode
	root := result.Tree.RootNode()

	classTypes := getClassNodeTypes(result.Language)

	Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		for _, ct := range classTypes {
			if node.Type() == ct {
				cls := extractClass(node, source, result.Language)
				if cls != nil {
					classes = append(classes, *cls)
				}
				return false // Don't descend into class body here
			}
		}
		return true
	})

	return classes
}

// getClassNodeTypes returns the AST node types for classes in each language.
func getClassNodeTypes(lang Language) []string {
	switch lang {
	case LangGo:
		return []string{"type_declaration"} // struct types
	case LangRust:
		return []string{"struct_item", "impl_item"}
	case LangPython:
		return []string{"class_definition"}
	case LangTypeScript, LangJavaScript, LangTSX:
		return []string{"class_declaration", "class"}
	case LangJava:
		return []string{"class_declaration", "interface_declaration"}
	case LangCPP:
		return []string{"class_specifier", "struct_specifier"}
	case LangCSharp:
		return []string{"class_declaration", "interface_declaration", "struct_declaration"}
	case LangRuby:
		return []string{"class", "module"}
	case LangPHP:
		return []string{"class_declaration", "interface_declaration", "trait_declaration"}
	default:
		return nil
	}
}

// extractClass extracts class details from an AST node.
func extractClass(node *sitter.Node, source []byte, lang Language) *ClassNode {
	cls := &ClassNode{
		StartLine: node.StartPoint().Row + 1,
		EndLine:   node.EndPoint().Row + 1,
	}

	// Extract class name
	if nameNode := node.ChildByFieldName("name"); nameNode != nil {
		cls.Name = GetNodeText(nameNode, source)
	}

	return cls
}
