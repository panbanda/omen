package semantic

import (
	"embed"
	"regexp"

	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

//go:embed ts_queries/*.scm
var tsQueryFiles embed.FS

type tsExtractor struct {
	funcValuesQuery  *sitter.Query
	decoratorsQuery  *sitter.Query
	dynamicCallQuery *sitter.Query
}

func newTypeScriptExtractor() *tsExtractor {
	lang, err := parser.GetTreeSitterLanguage(parser.LangTypeScript)
	if err != nil || lang == nil {
		return &tsExtractor{}
	}

	e := &tsExtractor{}

	funcSrc, err := tsQueryFiles.ReadFile("ts_queries/function_values.scm")
	if err == nil {
		q, err := sitter.NewQuery(funcSrc, lang)
		if err == nil {
			e.funcValuesQuery = q
		}
	}

	decSrc, err := tsQueryFiles.ReadFile("ts_queries/decorators.scm")
	if err == nil {
		q, err := sitter.NewQuery(decSrc, lang)
		if err == nil {
			e.decoratorsQuery = q
		}
	}

	dynSrc, err := tsQueryFiles.ReadFile("ts_queries/dynamic_calls.scm")
	if err == nil {
		q, err := sitter.NewQuery(dynSrc, lang)
		if err == nil {
			e.dynamicCallQuery = q
		}
	}

	return e
}

func (e *tsExtractor) ExtractRefs(tree *sitter.Tree, src []byte) []Ref {
	if tree == nil {
		return nil
	}

	var refs []Ref
	seen := make(map[string]bool)

	// Extract function values
	if e.funcValuesQuery != nil {
		qc := sitter.NewQueryCursor()
		qc.Exec(e.funcValuesQuery, tree.RootNode())

		for {
			match, ok := qc.NextMatch()
			if !ok {
				break
			}

			for _, cap := range match.Captures {
				capName := e.funcValuesQuery.CaptureNameForId(cap.Index)
				if capName == "func_ref" {
					name := string(src[cap.Node.StartByte():cap.Node.EndByte()])
					if tsIdentifierRegex.MatchString(name) && !tsBuiltins[name] && !seen[name] {
						seen[name] = true
						refs = append(refs, Ref{Name: name, Kind: RefFunctionValue})
					}
				}
			}
		}
		qc.Close()
	}

	// Extract decorated methods
	if e.decoratorsQuery != nil {
		qc := sitter.NewQueryCursor()
		qc.Exec(e.decoratorsQuery, tree.RootNode())

		for {
			match, ok := qc.NextMatch()
			if !ok {
				break
			}

			for _, cap := range match.Captures {
				capName := e.decoratorsQuery.CaptureNameForId(cap.Index)
				if capName == "decorated_method" {
					name := string(src[cap.Node.StartByte():cap.Node.EndByte()])
					if name != "" && !seen[name] {
						seen[name] = true
						refs = append(refs, Ref{Name: name, Kind: RefDecorator})
					}
				}
			}
		}
		qc.Close()
	}

	// Extract dynamic calls
	if e.dynamicCallQuery != nil {
		qc := sitter.NewQueryCursor()
		qc.Exec(e.dynamicCallQuery, tree.RootNode())

		for {
			match, ok := qc.NextMatch()
			if !ok {
				break
			}

			for _, cap := range match.Captures {
				capName := e.dynamicCallQuery.CaptureNameForId(cap.Index)
				if capName == "method_name" {
					name := string(src[cap.Node.StartByte():cap.Node.EndByte()])
					if tsIdentifierRegex.MatchString(name) && !seen[name] {
						seen[name] = true
						refs = append(refs, Ref{Name: name, Kind: RefDynamicCall})
					}
				}
			}
		}
		qc.Close()
	}

	return refs
}

func (e *tsExtractor) Close() {
	if e.funcValuesQuery != nil {
		e.funcValuesQuery.Close()
		e.funcValuesQuery = nil
	}
	if e.decoratorsQuery != nil {
		e.decoratorsQuery.Close()
		e.decoratorsQuery = nil
	}
	if e.dynamicCallQuery != nil {
		e.dynamicCallQuery.Close()
		e.dynamicCallQuery = nil
	}
}

var tsIdentifierRegex = regexp.MustCompile(`^[a-zA-Z_$][a-zA-Z0-9_$]*$`)

var tsBuiltins = map[string]bool{
	// Keywords
	"if": true, "else": true, "for": true, "while": true, "do": true,
	"switch": true, "case": true, "break": true, "continue": true, "return": true,
	"function": true, "class": true, "const": true, "let": true, "var": true,
	"new": true, "this": true, "super": true, "typeof": true, "instanceof": true,
	"void": true, "delete": true, "in": true, "of": true,
	"try": true, "catch": true, "finally": true, "throw": true,
	"async": true, "await": true, "yield": true,
	"import": true, "export": true, "default": true, "from": true, "as": true,
	"extends": true, "implements": true, "static": true, "public": true,
	"private": true, "protected": true, "readonly": true, "abstract": true,
	"interface": true, "type": true, "enum": true, "namespace": true,
	// Built-in values
	"true": true, "false": true, "null": true, "undefined": true,
	"NaN": true, "Infinity": true,
	// Common globals
	"console": true, "window": true, "document": true, "global": true,
	"process": true, "module": true, "exports": true, "require": true,
	"JSON": true, "Math": true, "Date": true, "Object": true, "Array": true,
	"String": true, "Number": true, "Boolean": true, "Symbol": true,
	"Promise": true, "Map": true, "Set": true, "WeakMap": true, "WeakSet": true,
	"Error": true, "TypeError": true, "ReferenceError": true,
	"setTimeout": true, "setInterval": true, "clearTimeout": true, "clearInterval": true,
}
