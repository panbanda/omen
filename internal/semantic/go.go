package semantic

import (
	"embed"

	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

//go:embed go_queries/*.scm
var goQueryFiles embed.FS

type goExtractor struct {
	funcValuesQuery *sitter.Query
}

func newGoExtractor() *goExtractor {
	lang, err := parser.GetTreeSitterLanguage(parser.LangGo)
	if err != nil || lang == nil {
		return &goExtractor{}
	}

	e := &goExtractor{}

	funcSrc, err := goQueryFiles.ReadFile("go_queries/function_values.scm")
	if err == nil {
		q, err := sitter.NewQuery(funcSrc, lang)
		if err == nil {
			e.funcValuesQuery = q
		}
	}

	return e
}

func (e *goExtractor) ExtractRefs(tree *sitter.Tree, src []byte) []Ref {
	if e.funcValuesQuery == nil || tree == nil {
		return nil
	}

	var refs []Ref
	seen := make(map[string]bool)

	qc := sitter.NewQueryCursor()
	defer qc.Close()
	qc.Exec(e.funcValuesQuery, tree.RootNode())

	for {
		match, ok := qc.NextMatch()
		if !ok {
			break
		}

		for _, cap := range match.Captures {
			capName := e.funcValuesQuery.CaptureNameForId(cap.Index)

			if capName == "func_ref" || capName == "method_ref" {
				name := string(src[cap.Node.StartByte():cap.Node.EndByte()])

				if goBuiltins[name] {
					continue
				}

				if name != "" && !seen[name] {
					seen[name] = true
					refs = append(refs, Ref{Name: name, Kind: RefFunctionValue})
				}
			}
		}
	}

	return refs
}

func (e *goExtractor) Close() {
	if e.funcValuesQuery != nil {
		e.funcValuesQuery.Close()
		e.funcValuesQuery = nil
	}
}

var goBuiltins = map[string]bool{
	// Built-in functions
	"append": true, "cap": true, "close": true, "complex": true,
	"copy": true, "delete": true, "imag": true, "len": true,
	"make": true, "new": true, "panic": true, "print": true,
	"println": true, "real": true, "recover": true, "clear": true,
	"min": true, "max": true,
	// Built-in types
	"bool": true, "byte": true, "complex64": true, "complex128": true,
	"error": true, "float32": true, "float64": true, "int": true,
	"int8": true, "int16": true, "int32": true, "int64": true,
	"rune": true, "string": true, "uint": true, "uint8": true,
	"uint16": true, "uint32": true, "uint64": true, "uintptr": true,
	"any": true, "comparable": true,
	// Constants
	"true": true, "false": true, "nil": true, "iota": true,
	// Common package names
	"fmt": true, "os": true, "io": true, "log": true, "http": true,
	"context": true, "errors": true, "strings": true, "bytes": true,
	"reflect": true, "time": true, "sync": true, "json": true,
}
