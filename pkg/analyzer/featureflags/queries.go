package featureflags

import (
	"embed"
	"fmt"
	"path/filepath"
	"regexp"
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

	// cachedRegexes holds pre-compiled regexes for #match? predicates.
	// Map: patternIndex -> stepIndex -> compiled regex
	cachedRegexes map[uint32]map[int]*regexp.Regexp
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

	// Pre-compile regexes for #match? and #not-match? predicates
	cachedRegexes := precompilePredicateRegexes(query)

	if r.queries[lang] == nil {
		r.queries[lang] = make(map[string]*QuerySet)
	}

	r.queries[lang][provider] = &QuerySet{
		Language:      lang,
		Provider:      provider,
		Query:         query,
		Pattern:       pattern,
		cachedRegexes: cachedRegexes,
	}

	return nil
}

// precompilePredicateRegexes extracts and compiles all regexes from #match? predicates.
func precompilePredicateRegexes(query *sitter.Query) map[uint32]map[int]*regexp.Regexp {
	result := make(map[uint32]map[int]*regexp.Regexp)

	patternCount := query.PatternCount()
	for patternIdx := uint32(0); patternIdx < patternCount; patternIdx++ {
		predicates := query.PredicatesForPattern(patternIdx)
		if len(predicates) == 0 {
			continue
		}

		patternRegexes := make(map[int]*regexp.Regexp)

		for stepIdx, steps := range predicates {
			if len(steps) < 3 {
				continue
			}

			operator := query.StringValueForId(steps[0].ValueId)
			if operator != "match?" && operator != "not-match?" {
				continue
			}

			// steps[2] contains the regex pattern
			regexPattern := query.StringValueForId(steps[2].ValueId)
			if re, err := regexp.Compile(regexPattern); err == nil {
				patternRegexes[stepIdx] = re
			}
		}

		if len(patternRegexes) > 0 {
			result[patternIdx] = patternRegexes
		}
	}

	return result
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

// FilterPredicates applies predicate filtering using pre-compiled regexes.
// This avoids recompiling regexes on every match, which is a significant performance win.
func (qs *QuerySet) FilterPredicates(m *sitter.QueryMatch, input []byte) *sitter.QueryMatch {
	q := qs.Query

	predicates := q.PredicatesForPattern(uint32(m.PatternIndex))
	if len(predicates) == 0 {
		return m
	}

	patternRegexes := qs.cachedRegexes[uint32(m.PatternIndex)]

	for stepIdx, steps := range predicates {
		if len(steps) < 3 {
			continue
		}

		operator := q.StringValueForId(steps[0].ValueId)

		switch operator {
		case "eq?", "not-eq?":
			isPositive := operator == "eq?"
			expectedCaptureNameLeft := q.CaptureNameForId(steps[1].ValueId)

			if steps[2].Type == sitter.QueryPredicateStepTypeCapture {
				expectedCaptureNameRight := q.CaptureNameForId(steps[2].ValueId)
				var nodeLeft, nodeRight *sitter.Node

				for _, c := range m.Captures {
					captureName := q.CaptureNameForId(c.Index)
					if captureName == expectedCaptureNameLeft {
						nodeLeft = c.Node
					}
					if captureName == expectedCaptureNameRight {
						nodeRight = c.Node
					}
					if nodeLeft != nil && nodeRight != nil {
						if (nodeLeft.Content(input) == nodeRight.Content(input)) != isPositive {
							return nil
						}
						break
					}
				}
			} else {
				expectedValueRight := q.StringValueForId(steps[2].ValueId)
				for _, c := range m.Captures {
					captureName := q.CaptureNameForId(c.Index)
					if expectedCaptureNameLeft != captureName {
						continue
					}
					if (c.Node.Content(input) == expectedValueRight) != isPositive {
						return nil
					}
				}
			}

		case "match?", "not-match?":
			isPositive := operator == "match?"
			expectedCaptureName := q.CaptureNameForId(steps[1].ValueId)

			var re *regexp.Regexp
			if patternRegexes != nil {
				re = patternRegexes[stepIdx]
			}
			if re == nil {
				regexPattern := q.StringValueForId(steps[2].ValueId)
				var err error
				re, err = regexp.Compile(regexPattern)
				if err != nil {
					return nil
				}
			}

			// Find the capture and check if it matches
			found := false
			for _, c := range m.Captures {
				captureName := q.CaptureNameForId(c.Index)
				if expectedCaptureName != captureName {
					continue
				}
				found = true
				if re.MatchString(c.Node.Content(input)) != isPositive {
					return nil
				}
			}
			// If the capture wasn't found, the predicate doesn't apply
			// This can happen when a pattern has optional captures
			_ = found
		}
	}

	return m
}
