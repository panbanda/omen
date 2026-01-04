package treesitter

import (
	"github.com/panbanda/omen/pkg/ast"
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// Provider implements ast.Provider using tree-sitter.
type Provider struct {
	parser *parser.Parser
}

// New creates a new tree-sitter based provider.
func New() *Provider {
	return &Provider{
		parser: parser.New(),
	}
}

// Parse parses a file and returns syntax-level information.
func (p *Provider) Parse(path string) (ast.File, error) {
	result, err := p.parser.ParseFile(path)
	if err != nil {
		return nil, err
	}
	return &file{result: result}, nil
}

// ParseWithTypes returns ErrTypesUnavailable for tree-sitter.
func (p *Provider) ParseWithTypes(path string) (ast.TypedFile, error) {
	return nil, ast.ErrTypesUnavailable
}

// Language returns the detected language for a file path.
func (p *Provider) Language(path string) ast.Language {
	lang := parser.DetectLanguage(path)
	return ast.Language(lang)
}

// Close releases parser resources.
func (p *Provider) Close() {
	p.parser.Close()
}

// file wraps a parser.ParseResult to implement ast.File.
type file struct {
	result *parser.ParseResult
}

func (f *file) Path() string {
	return f.result.Path
}

func (f *file) Language() ast.Language {
	return ast.Language(f.result.Language)
}

func (f *file) Functions() []ast.FunctionDecl {
	fns := parser.GetFunctions(f.result)
	result := make([]ast.FunctionDecl, len(fns))
	for i, fn := range fns {
		result[i] = ast.FunctionDecl{
			Name:      fn.Name,
			Signature: fn.Signature,
			Pos: ast.Position{
				File: f.result.Path,
				Line: int(fn.StartLine),
			},
			EndLine: int(fn.EndLine),
		}
	}
	return result
}

func (f *file) Calls() []ast.CallInfo {
	calls := extractCalls(f.result)
	return calls
}

func (f *file) Symbols() []ast.Symbol {
	var symbols []ast.Symbol

	// Add functions as symbols
	for _, fn := range f.Functions() {
		symbols = append(symbols, ast.Symbol{
			Name: fn.Name,
			Kind: ast.SymbolFunction,
			Pos:  fn.Pos,
		})
	}

	// Add classes as symbols
	classes := parser.GetClasses(f.result)
	for _, cls := range classes {
		symbols = append(symbols, ast.Symbol{
			Name: cls.Name,
			Kind: ast.SymbolClass,
			Pos: ast.Position{
				File: f.result.Path,
				Line: int(cls.StartLine),
			},
		})
	}

	return symbols
}

func (f *file) Imports() []ast.Import {
	imports := extractImports(f.result)
	return imports
}

// extractCalls walks the AST to find function/method calls.
func extractCalls(result *parser.ParseResult) []ast.CallInfo {
	var calls []ast.CallInfo
	root := result.Tree.RootNode()

	callTypes := getCallNodeTypes(result.Language)
	if len(callTypes) == 0 {
		return nil
	}

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		nodeType := node.Type()
		for _, ct := range callTypes {
			if nodeType == ct {
				call := extractCall(node, source, result.Language, result.Path)
				if call != nil {
					calls = append(calls, *call)
				}
				break
			}
		}
		return true
	})

	return calls
}

// getCallNodeTypes returns AST node types for function calls by language.
func getCallNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangGo:
		return []string{"call_expression"}
	case parser.LangRust:
		return []string{"call_expression", "method_call_expression"}
	case parser.LangPython:
		return []string{"call"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"call_expression"}
	case parser.LangJava:
		return []string{"method_invocation"}
	case parser.LangC, parser.LangCPP:
		return []string{"call_expression"}
	case parser.LangCSharp:
		return []string{"invocation_expression"}
	case parser.LangRuby:
		return []string{"call", "method_call"}
	case parser.LangPHP:
		return []string{"function_call_expression", "method_call_expression"}
	default:
		return nil
	}
}

// extractCall extracts call information from a call expression node.
func extractCall(node *sitter.Node, source []byte, lang parser.Language, path string) *ast.CallInfo {
	call := &ast.CallInfo{
		Pos: ast.Position{
			File:   path,
			Line:   int(node.StartPoint().Row) + 1,
			Column: int(node.StartPoint().Column) + 1,
			Offset: int(node.StartByte()),
		},
	}

	switch lang {
	case parser.LangGo:
		if fnNode := node.ChildByFieldName("function"); fnNode != nil {
			call.Callee = parser.GetNodeText(fnNode, source)
			// Check if it's a method call (selector expression)
			if fnNode.Type() == "selector_expression" {
				if operand := fnNode.ChildByFieldName("operand"); operand != nil {
					call.Receiver = parser.GetNodeText(operand, source)
				}
				if field := fnNode.ChildByFieldName("field"); field != nil {
					call.Callee = parser.GetNodeText(field, source)
				}
			}
		}
		if args := node.ChildByFieldName("arguments"); args != nil {
			call.Args = countArgs(args)
		}

	case parser.LangPython:
		if fnNode := node.ChildByFieldName("function"); fnNode != nil {
			call.Callee = parser.GetNodeText(fnNode, source)
			if fnNode.Type() == "attribute" {
				if obj := fnNode.ChildByFieldName("object"); obj != nil {
					call.Receiver = parser.GetNodeText(obj, source)
				}
				if attr := fnNode.ChildByFieldName("attribute"); attr != nil {
					call.Callee = parser.GetNodeText(attr, source)
				}
			}
		}
		if args := node.ChildByFieldName("arguments"); args != nil {
			call.Args = countArgs(args)
		}

	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		if fnNode := node.ChildByFieldName("function"); fnNode != nil {
			call.Callee = parser.GetNodeText(fnNode, source)
			if fnNode.Type() == "member_expression" {
				if obj := fnNode.ChildByFieldName("object"); obj != nil {
					call.Receiver = parser.GetNodeText(obj, source)
				}
				if prop := fnNode.ChildByFieldName("property"); prop != nil {
					call.Callee = parser.GetNodeText(prop, source)
				}
			}
		}
		if args := node.ChildByFieldName("arguments"); args != nil {
			call.Args = countArgs(args)
		}

	default:
		// Generic extraction for other languages
		call.Callee = parser.GetNodeText(node, source)
	}

	if call.Callee == "" {
		return nil
	}

	return call
}

// countArgs counts the number of arguments in an argument list node.
func countArgs(argsNode *sitter.Node) int {
	count := 0
	for i := range int(argsNode.ChildCount()) {
		child := argsNode.Child(i)
		// Skip punctuation (parentheses, commas)
		if child.IsNamed() {
			count++
		}
	}
	return count
}

// extractImports walks the AST to find import statements.
func extractImports(result *parser.ParseResult) []ast.Import {
	var imports []ast.Import
	root := result.Tree.RootNode()

	importTypes := getImportNodeTypes(result.Language)
	if len(importTypes) == 0 {
		return nil
	}

	parser.Walk(root, result.Source, func(node *sitter.Node, source []byte) bool {
		nodeType := node.Type()
		for _, it := range importTypes {
			if nodeType == it {
				extracted := extractImport(node, source, result.Language, result.Path)
				imports = append(imports, extracted...)
				break
			}
		}
		return true
	})

	return imports
}

// getImportNodeTypes returns AST node types for imports by language.
func getImportNodeTypes(lang parser.Language) []string {
	switch lang {
	case parser.LangGo:
		return []string{"import_declaration"}
	case parser.LangRust:
		return []string{"use_declaration"}
	case parser.LangPython:
		return []string{"import_statement", "import_from_statement"}
	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		return []string{"import_statement"}
	case parser.LangJava:
		return []string{"import_declaration"}
	case parser.LangCSharp:
		return []string{"using_directive"}
	case parser.LangRuby:
		return []string{"call"} // require/require_relative
	case parser.LangPHP:
		return []string{"namespace_use_declaration"}
	default:
		return nil
	}
}

// extractImport extracts import information from an import node.
func extractImport(node *sitter.Node, source []byte, lang parser.Language, path string) []ast.Import {
	var imports []ast.Import

	pos := ast.Position{
		File:   path,
		Line:   int(node.StartPoint().Row) + 1,
		Column: int(node.StartPoint().Column) + 1,
		Offset: int(node.StartByte()),
	}

	switch lang {
	case parser.LangGo:
		// Handle both single imports and import blocks
		parser.Walk(node, source, func(n *sitter.Node, src []byte) bool {
			if n.Type() == "import_spec" {
				imp := ast.Import{Pos: pos}
				if pathNode := n.ChildByFieldName("path"); pathNode != nil {
					// Remove quotes from import path
					imp.Path = trimQuotes(parser.GetNodeText(pathNode, src))
				}
				if nameNode := n.ChildByFieldName("name"); nameNode != nil {
					imp.Alias = parser.GetNodeText(nameNode, src)
				}
				if imp.Path != "" {
					imports = append(imports, imp)
				}
			}
			return true
		})

	case parser.LangPython:
		if node.Type() == "import_statement" {
			// import module
			parser.Walk(node, source, func(n *sitter.Node, src []byte) bool {
				nodeType := n.Type()
				if nodeType == "dotted_name" || nodeType == "aliased_import" {
					imp := ast.Import{Pos: pos}
					if nodeType == "aliased_import" {
						if nameNode := n.ChildByFieldName("name"); nameNode != nil {
							imp.Path = parser.GetNodeText(nameNode, src)
						}
						if aliasNode := n.ChildByFieldName("alias"); aliasNode != nil {
							imp.Alias = parser.GetNodeText(aliasNode, src)
						}
					} else {
						imp.Path = parser.GetNodeText(n, src)
					}
					if imp.Path != "" {
						imports = append(imports, imp)
					}
				}
				return true
			})
		} else {
			// from module import ...
			if moduleNode := node.ChildByFieldName("module_name"); moduleNode != nil {
				imp := ast.Import{
					Path: parser.GetNodeText(moduleNode, source),
					Pos:  pos,
				}
				imports = append(imports, imp)
			}
		}

	case parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX:
		if srcNode := node.ChildByFieldName("source"); srcNode != nil {
			imp := ast.Import{
				Path: trimQuotes(parser.GetNodeText(srcNode, source)),
				Pos:  pos,
			}
			imports = append(imports, imp)
		}

	case parser.LangJava:
		parser.Walk(node, source, func(n *sitter.Node, src []byte) bool {
			if n.Type() == "scoped_identifier" {
				imp := ast.Import{
					Path: parser.GetNodeText(n, src),
					Pos:  pos,
				}
				imports = append(imports, imp)
				return false
			}
			return true
		})

	default:
		// Generic: just grab the full text
		imp := ast.Import{
			Path: parser.GetNodeText(node, source),
			Pos:  pos,
		}
		if imp.Path != "" {
			imports = append(imports, imp)
		}
	}

	return imports
}

// trimQuotes removes surrounding quotes from a string.
func trimQuotes(s string) string {
	if len(s) >= 2 {
		if (s[0] == '"' && s[len(s)-1] == '"') ||
			(s[0] == '\'' && s[len(s)-1] == '\'') ||
			(s[0] == '`' && s[len(s)-1] == '`') {
			return s[1 : len(s)-1]
		}
	}
	return s
}
