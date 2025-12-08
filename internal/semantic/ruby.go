package semantic

import (
	"embed"
	"regexp"
	"strings"

	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

//go:embed ruby_queries/*.scm
var rubyQueryFiles embed.FS

type rubyExtractor struct {
	callbackQuery     *sitter.Query
	dynamicCallsQuery *sitter.Query
}

func newRubyExtractor() *rubyExtractor {
	lang, err := parser.GetTreeSitterLanguage(parser.LangRuby)
	if err != nil || lang == nil {
		return &rubyExtractor{}
	}

	e := &rubyExtractor{}

	callbackSrc, err := rubyQueryFiles.ReadFile("ruby_queries/callbacks.scm")
	if err == nil {
		q, err := sitter.NewQuery(callbackSrc, lang)
		if err == nil {
			e.callbackQuery = q
		}
	}

	dynamicSrc, err := rubyQueryFiles.ReadFile("ruby_queries/dynamic_calls.scm")
	if err == nil {
		q, err := sitter.NewQuery(dynamicSrc, lang)
		if err == nil {
			e.dynamicCallsQuery = q
		}
	}

	return e
}

func (e *rubyExtractor) ExtractRefs(tree *sitter.Tree, src []byte) []Ref {
	if tree == nil {
		return nil
	}

	var refs []Ref
	seen := make(map[string]bool)

	// Extract callbacks
	if e.callbackQuery != nil {
		qc := sitter.NewQueryCursor()
		qc.Exec(e.callbackQuery, tree.RootNode())

		for {
			match, ok := qc.NextMatch()
			if !ok {
				break
			}

			match = qc.FilterPredicates(match, src)
			if len(match.Captures) == 0 {
				continue
			}

			for _, cap := range match.Captures {
				capName := e.callbackQuery.CaptureNameForId(cap.Index)

				switch capName {
				case "method_name", "condition_method", "scope_name", "attr_name", "delegate_name", "custom_validator":
					name := extractRubySymbolName(cap.Node, src)
					if name != "" && !seen[name] {
						seen[name] = true
						refs = append(refs, Ref{Name: name, Kind: RefCallback})
					}
				}
			}
		}
		qc.Close()
	}

	// Extract dynamic calls
	if e.dynamicCallsQuery != nil {
		qc := sitter.NewQueryCursor()
		qc.Exec(e.dynamicCallsQuery, tree.RootNode())

		for {
			match, ok := qc.NextMatch()
			if !ok {
				break
			}

			match = qc.FilterPredicates(match, src)
			if len(match.Captures) == 0 {
				continue
			}

			for _, cap := range match.Captures {
				capName := e.dynamicCallsQuery.CaptureNameForId(cap.Index)

				switch capName {
				case "method_name", "new_name", "old_name":
					name := extractRubySymbolName(cap.Node, src)
					if name != "" && !seen[name] {
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

func (e *rubyExtractor) Close() {
	if e.callbackQuery != nil {
		e.callbackQuery.Close()
		e.callbackQuery = nil
	}
	if e.dynamicCallsQuery != nil {
		e.dynamicCallsQuery.Close()
		e.dynamicCallsQuery = nil
	}
}

func extractRubySymbolName(node *sitter.Node, src []byte) string {
	if node == nil {
		return ""
	}

	text := string(src[node.StartByte():node.EndByte()])

	// Handle simple_symbol: :method_name -> method_name
	if strings.HasPrefix(text, ":") {
		return text[1:]
	}

	// Remove quotes if present
	text = strings.Trim(text, `"'`)

	if rubyMethodNameRegex.MatchString(text) {
		return text
	}

	return ""
}

var rubyMethodNameRegex = regexp.MustCompile(`^[a-zA-Z_][a-zA-Z0-9_]*[?!=]?$`)
