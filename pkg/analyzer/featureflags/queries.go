package featureflags

import (
	"embed"
	"fmt"
	"path/filepath"
	"strings"

	sitter "github.com/smacker/go-tree-sitter"

	"github.com/panbanda/omen/pkg/parser"
)

//go:embed queries/*/*.scm
var queryFiles embed.FS

// QuerySet holds compiled tree-sitter queries for a specific language.
type QuerySet struct {
	Language parser.Language
	Provider string
	Query    *sitter.Query
	Pattern  string // Original pattern for debugging
}

// QueryRegistry manages all loaded queries organized by language and provider.
type QueryRegistry struct {
	// queries maps language -> provider -> QuerySet
	queries map[parser.Language]map[string]*QuerySet
}

// NewQueryRegistry creates and initializes a query registry with embedded queries.
func NewQueryRegistry() (*QueryRegistry, error) {
	r := &QueryRegistry{
		queries: make(map[parser.Language]map[string]*QuerySet),
	}

	if err := r.loadEmbeddedQueries(); err != nil {
		return nil, fmt.Errorf("loading embedded queries: %w", err)
	}

	return r, nil
}

// loadEmbeddedQueries loads all .scm files from the embedded filesystem.
func (r *QueryRegistry) loadEmbeddedQueries() error {
	entries, err := queryFiles.ReadDir("queries")
	if err != nil {
		return fmt.Errorf("reading queries directory: %w", err)
	}

	for _, langDir := range entries {
		if !langDir.IsDir() {
			continue
		}

		langName := langDir.Name()
		lang := dirNameToLanguage(langName)
		if lang == parser.LangUnknown {
			continue
		}

		providerFiles, err := queryFiles.ReadDir(filepath.Join("queries", langName))
		if err != nil {
			continue
		}

		for _, pf := range providerFiles {
			if pf.IsDir() || !strings.HasSuffix(pf.Name(), ".scm") {
				continue
			}

			provider := strings.TrimSuffix(pf.Name(), ".scm")
			pattern, err := queryFiles.ReadFile(filepath.Join("queries", langName, pf.Name()))
			if err != nil {
				continue
			}

			if err := r.AddQuery(lang, provider, string(pattern)); err != nil {
				// Log but don't fail - allow partial loading
				continue
			}

			// For JavaScript queries, also register for TypeScript and TSX
			// since they share similar AST patterns
			if lang == parser.LangJavaScript {
				// Try to add for TypeScript (may fail if patterns don't match)
				_ = r.AddQuery(parser.LangTypeScript, provider, string(pattern))
				_ = r.AddQuery(parser.LangTSX, provider, string(pattern))
			}
		}
	}

	return nil
}

// AddQuery compiles and adds a query pattern for a language/provider combination.
func (r *QueryRegistry) AddQuery(lang parser.Language, provider, pattern string) error {
	tsLang, err := parser.GetTreeSitterLanguage(lang)
	if err != nil {
		return fmt.Errorf("getting tree-sitter language %s: %w", lang, err)
	}

	query, err := sitter.NewQuery([]byte(pattern), tsLang)
	if err != nil {
		return fmt.Errorf("compiling query for %s/%s: %w", lang, provider, err)
	}

	if r.queries[lang] == nil {
		r.queries[lang] = make(map[string]*QuerySet)
	}

	r.queries[lang][provider] = &QuerySet{
		Language: lang,
		Provider: provider,
		Query:    query,
		Pattern:  pattern,
	}

	return nil
}

// GetQueries returns all queries for a language, optionally filtered by providers.
func (r *QueryRegistry) GetQueries(lang parser.Language, providers []string) []*QuerySet {
	// First try to get queries compiled for this specific language
	langQueries, ok := r.queries[lang]
	if !ok {
		// Fall back to normalized language (e.g., TypeScript -> JavaScript)
		// This handles cases where only JavaScript queries exist
		normalizedLang := normalizeLanguageForQueries(lang)
		if normalizedLang != lang {
			langQueries, ok = r.queries[normalizedLang]
		}
		if !ok {
			return nil
		}
	}

	if len(providers) == 0 {
		// Return all providers for this language
		result := make([]*QuerySet, 0, len(langQueries))
		for _, qs := range langQueries {
			result = append(result, qs)
		}
		return result
	}

	// Filter by specified providers
	providerSet := make(map[string]bool)
	for _, p := range providers {
		providerSet[strings.ToLower(p)] = true
	}

	result := make([]*QuerySet, 0)
	for name, qs := range langQueries {
		if providerSet[strings.ToLower(name)] {
			result = append(result, qs)
		}
	}
	return result
}

// GetAllLanguages returns all languages that have queries registered.
func (r *QueryRegistry) GetAllLanguages() []parser.Language {
	langs := make([]parser.Language, 0, len(r.queries))
	for lang := range r.queries {
		langs = append(langs, lang)
	}
	return langs
}

// GetProviders returns all providers registered for a language.
func (r *QueryRegistry) GetProviders(lang parser.Language) []string {
	langQueries, ok := r.queries[lang]
	if !ok {
		return nil
	}

	providers := make([]string, 0, len(langQueries))
	for name := range langQueries {
		providers = append(providers, name)
	}
	return providers
}

// Close releases all query resources.
func (r *QueryRegistry) Close() {
	for _, langQueries := range r.queries {
		for _, qs := range langQueries {
			if qs.Query != nil {
				qs.Query.Close()
			}
		}
	}
	r.queries = nil
}

// dirNameToLanguage maps directory names to parser.Language constants.
func dirNameToLanguage(name string) parser.Language {
	switch strings.ToLower(name) {
	case "javascript", "js", "typescript", "ts":
		return parser.LangJavaScript // JS and TS share the same queries
	case "python", "py":
		return parser.LangPython
	case "go", "golang":
		return parser.LangGo
	case "java":
		return parser.LangJava
	case "ruby", "rb":
		return parser.LangRuby
	default:
		return parser.LangUnknown
	}
}

// LanguageToDirName maps parser.Language to directory name for custom queries.
func LanguageToDirName(lang parser.Language) string {
	switch lang {
	case parser.LangJavaScript, parser.LangTypeScript, parser.LangTSX:
		return "javascript"
	case parser.LangPython:
		return "python"
	case parser.LangGo:
		return "go"
	case parser.LangJava:
		return "java"
	case parser.LangRuby:
		return "ruby"
	default:
		return ""
	}
}

// LoadCustomProvider loads a custom provider's query for specified languages.
func (r *QueryRegistry) LoadCustomProvider(name string, languages []string, pattern string) error {
	for _, langName := range languages {
		lang := dirNameToLanguage(langName)
		if lang == parser.LangUnknown {
			continue
		}

		if err := r.AddQuery(lang, name, pattern); err != nil {
			return fmt.Errorf("adding custom query for %s/%s: %w", langName, name, err)
		}

		// For JavaScript queries, also register for TypeScript and TSX
		if lang == parser.LangJavaScript {
			_ = r.AddQuery(parser.LangTypeScript, name, pattern)
			_ = r.AddQuery(parser.LangTSX, name, pattern)
		}
	}
	return nil
}

// normalizeLanguageForQueries maps related languages to their query directory.
// TypeScript and TSX use the JavaScript query files.
func normalizeLanguageForQueries(lang parser.Language) parser.Language {
	switch lang {
	case parser.LangTypeScript, parser.LangTSX:
		return parser.LangJavaScript
	default:
		return lang
	}
}
