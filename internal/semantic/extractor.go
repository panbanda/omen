package semantic

import (
	"github.com/panbanda/omen/pkg/parser"
	sitter "github.com/smacker/go-tree-sitter"
)

// RefKind categorizes how a function/method is indirectly referenced.
type RefKind int

const (
	RefCallback      RefKind = iota // Framework callback (e.g., before_save :method)
	RefDecorator                    // Decorated method (e.g., @Get() handler())
	RefFunctionValue                // Function used as value (e.g., handler: processRequest)
	RefDynamicCall                  // Dynamic dispatch (e.g., send(:method), obj["method"]())
)

func (k RefKind) String() string {
	switch k {
	case RefCallback:
		return "callback"
	case RefDecorator:
		return "decorator"
	case RefFunctionValue:
		return "function_value"
	case RefDynamicCall:
		return "dynamic_call"
	default:
		return "unknown"
	}
}

// Ref represents an indirect reference to a function or method.
type Ref struct {
	Name string
	Kind RefKind
}

// Extractor finds indirect function references that bypass normal call graphs.
// These are references created by framework patterns, decorators, dynamic dispatch,
// or other language idioms where a function is "called" without an explicit call site.
type Extractor interface {
	// ExtractRefs returns function/method references found indirectly.
	// This includes callbacks, decorators, dynamic dispatch, and similar patterns.
	ExtractRefs(tree *sitter.Tree, src []byte) []Ref

	// Close releases resources held by the extractor (compiled queries, etc.)
	Close()
}

// ForLanguage returns an Extractor for the given language, or nil if none exists.
func ForLanguage(lang parser.Language) Extractor {
	switch lang {
	case parser.LangRuby:
		return newRubyExtractor()
	case parser.LangGo:
		return newGoExtractor()
	case parser.LangTypeScript, parser.LangTSX, parser.LangJavaScript:
		return newTypeScriptExtractor()
	default:
		return nil
	}
}

// SupportedLanguages returns languages with Extractor implementations.
func SupportedLanguages() []parser.Language {
	return []parser.Language{
		parser.LangRuby,
		parser.LangGo,
		parser.LangTypeScript,
		parser.LangTSX,
		parser.LangJavaScript,
	}
}
