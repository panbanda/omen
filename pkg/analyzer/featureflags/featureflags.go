package featureflags

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	sitter "github.com/smacker/go-tree-sitter"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
)

// Compile-time check that Analyzer implements analyzer.FileAnalyzer.
var _ analyzer.FileAnalyzer[*Analysis] = (*Analyzer)(nil)

// Analyzer detects feature flags in source code using tree-sitter queries.
type Analyzer struct {
	parser          *parser.Parser
	registry        *QueryRegistry
	providers       []string
	customProviders []CustomProvider
	maxFileSize     int64
	vcsOpener       vcs.Opener
	includeGit      bool
	expectedTTL     int // days
}

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithProviders filters detection to specific providers.
func WithProviders(providers []string) Option {
	return func(a *Analyzer) {
		a.providers = providers
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// WithVCSOpener sets the VCS opener for git history analysis.
func WithVCSOpener(opener vcs.Opener) Option {
	return func(a *Analyzer) {
		a.vcsOpener = opener
	}
}

// WithGitHistory enables git history analysis for staleness detection.
func WithGitHistory(include bool) Option {
	return func(a *Analyzer) {
		a.includeGit = include
	}
}

// WithExpectedTTL sets the expected time-to-live for flags in days.
func WithExpectedTTL(days int) Option {
	return func(a *Analyzer) {
		a.expectedTTL = days
	}
}

// CustomProvider defines a custom feature flag provider configuration.
type CustomProvider struct {
	Name      string
	Languages []string
	Query     string
}

// WithCustomProviders registers custom feature flag providers.
func WithCustomProviders(providers []CustomProvider) Option {
	return func(a *Analyzer) {
		a.customProviders = providers
	}
}

// New creates a new feature flag analyzer.
func New(opts ...Option) (*Analyzer, error) {
	registry, err := NewQueryRegistry()
	if err != nil {
		return nil, err
	}

	a := &Analyzer{
		parser:      parser.New(),
		registry:    registry,
		providers:   nil, // nil means all providers
		maxFileSize: 0,
		vcsOpener:   vcs.DefaultOpener(),
		includeGit:  true,
		expectedTTL: 14, // default 14 days for release flags
	}

	for _, opt := range opts {
		opt(a)
	}

	// Load custom providers after options are applied
	for _, cp := range a.customProviders {
		if err := registry.LoadCustomProvider(cp.Name, cp.Languages, cp.Query); err != nil {
			return nil, fmt.Errorf("loading custom provider %s: %w", cp.Name, err)
		}
	}

	return a, nil
}

// Close releases analyzer resources.
func (a *Analyzer) Close() {
	if a.parser != nil {
		a.parser.Close()
	}
	if a.registry != nil {
		a.registry.Close()
	}
}

// AnalyzeFile analyzes a single file for feature flags.
func (a *Analyzer) AnalyzeFile(path string) ([]Reference, error) {
	return a.analyzeFileWithParser(a.parser, path)
}

// analyzeFileWithParser analyzes a file using the provided parser instance.
func (a *Analyzer) analyzeFileWithParser(p *parser.Parser, path string) ([]Reference, error) {
	var result *parser.ParseResult
	var err error

	if a.maxFileSize > 0 {
		result, err = p.ParseFileWithLimit(path, a.maxFileSize)
	} else {
		result, err = p.ParseFile(path)
	}

	if err != nil {
		return nil, err
	}

	if result == nil || result.Tree == nil {
		return nil, nil
	}

	queries := a.registry.GetQueries(result.Language, a.providers)
	if len(queries) == 0 {
		return nil, nil
	}

	var refs []Reference

	for _, qs := range queries {
		matches := a.executeQuery(qs, result)
		for _, m := range matches {
			m.File = path
			refs = append(refs, m)
		}
	}

	// Calculate nesting depth for all references in a single AST walk
	if len(refs) > 0 {
		lines := make([]uint32, len(refs))
		for i, ref := range refs {
			lines[i] = ref.Line
		}
		depths := a.calculateNestingDepthBatch(result, lines)
		for i := range refs {
			refs[i].NestingDepth = depths[refs[i].Line]
		}
	}

	// Deduplicate references by (file, line, column, flagKey)
	refs = deduplicateRefs(refs)

	return refs, nil
}

// deduplicateRefs removes duplicate references based on location.
func deduplicateRefs(refs []Reference) []Reference {
	seen := make(map[string]bool)
	result := make([]Reference, 0, len(refs))
	for _, ref := range refs {
		key := fmt.Sprintf("%s:%d:%d:%s", ref.File, ref.Line, ref.Column, ref.FlagKey)
		if !seen[key] {
			seen[key] = true
			result = append(result, ref)
		}
	}
	return result
}

// executeQuery runs a tree-sitter query and extracts flag references.
func (a *Analyzer) executeQuery(qs *QuerySet, result *parser.ParseResult) []Reference {
	cursor := sitter.NewQueryCursor()
	defer cursor.Close()

	cursor.Exec(qs.Query, result.Tree.RootNode())

	var refs []Reference

	for {
		match, ok := cursor.NextMatch()
		if !ok {
			break
		}

		// Apply predicates using cached regexes to avoid recompilation overhead
		match = qs.FilterPredicates(match, result.Source)
		if match == nil {
			continue
		}

		var flagKey string
		var line, column uint32

		for _, capture := range match.Captures {
			captureName := qs.Query.CaptureNameForId(capture.Index)
			if captureName == "flag_key" {
				flagKey = extractFlagKey(capture.Node, result.Source)
				line = uint32(capture.Node.StartPoint().Row) + 1
				column = uint32(capture.Node.StartPoint().Column)
			}
		}

		if flagKey != "" {
			refs = append(refs, Reference{
				FlagKey:  flagKey,
				Provider: qs.Provider,
				Line:     line,
				Column:   column,
			})
		}
	}

	return refs
}

// extractFlagKey extracts the flag key string from a node, handling quotes.
func extractFlagKey(node *sitter.Node, source []byte) string {
	text := parser.GetNodeText(node, source)
	// Remove surrounding quotes if present
	text = strings.Trim(text, "\"'`:") // handle "key", 'key', :key (Ruby symbol)
	return text
}

// conditionalNodeTypesCache holds pre-computed conditional node types per language.
var conditionalNodeTypesCache = func() map[parser.Language]map[string]bool {
	cache := make(map[parser.Language]map[string]bool)

	// Common types shared across languages
	common := []string{"if_statement", "if_expression", "switch_statement"}

	// Language-specific types
	langTypes := map[parser.Language][]string{
		parser.LangGo:         {"expression_switch_statement", "type_switch_statement", "expression_case", "type_case"},
		parser.LangPython:     {"match_statement", "case_clause", "conditional_expression"},
		parser.LangJavaScript: {"switch_case", "ternary_expression"},
		parser.LangTypeScript: {"switch_case", "ternary_expression"},
		parser.LangTSX:        {"switch_case", "ternary_expression"},
		parser.LangJava:       {"switch_expression", "switch_block_statement_group", "ternary_expression"},
		parser.LangRuby:       {"if", "unless", "case", "when", "conditional"},
		parser.LangC:          {"case_statement"},
		parser.LangCPP:        {"case_statement"},
		parser.LangCSharp:     {"switch_section", "conditional_expression"},
	}

	// Build cache for each language
	for lang, types := range langTypes {
		m := make(map[string]bool, len(common)+len(types))
		for _, t := range common {
			m[t] = true
		}
		for _, t := range types {
			m[t] = true
		}
		cache[lang] = m
	}

	// Default for languages without specific types
	defaultTypes := make(map[string]bool, len(common))
	for _, t := range common {
		defaultTypes[t] = true
	}
	cache[parser.Language("")] = defaultTypes

	return cache
}()

// conditionalNodeTypes returns the set of AST node types that represent conditional constructs
// for a specific language. This is used for nesting depth calculation.
func conditionalNodeTypes(lang parser.Language) map[string]bool {
	if types, ok := conditionalNodeTypesCache[lang]; ok {
		return types
	}
	return conditionalNodeTypesCache[parser.Language("")]
}

// calculateNestingDepth determines how deeply nested a line is within conditionals.
func (a *Analyzer) calculateNestingDepth(result *parser.ParseResult, line uint32) int {
	depth := 0
	conditionalTypes := conditionalNodeTypes(result.Language)

	parser.Walk(result.Tree.RootNode(), result.Source, func(node *sitter.Node, source []byte) bool {
		startLine := uint32(node.StartPoint().Row) + 1
		endLine := uint32(node.EndPoint().Row) + 1

		if conditionalTypes[node.Type()] && startLine <= line && line <= endLine {
			depth++
		}
		return true
	})

	return depth
}

// calculateNestingDepthBatch calculates nesting depths for multiple lines in a single AST walk.
// This is more efficient than calling calculateNestingDepth for each line individually.
func (a *Analyzer) calculateNestingDepthBatch(result *parser.ParseResult, lines []uint32) map[uint32]int {
	if len(lines) == 0 {
		return make(map[uint32]int)
	}

	// Deduplicate lines to avoid counting the same line multiple times
	depths := make(map[uint32]int, len(lines))
	for _, line := range lines {
		depths[line] = 0
	}

	conditionalTypes := conditionalNodeTypes(result.Language)

	parser.Walk(result.Tree.RootNode(), result.Source, func(node *sitter.Node, source []byte) bool {
		if !conditionalTypes[node.Type()] {
			return true
		}

		startLine := uint32(node.StartPoint().Row) + 1
		endLine := uint32(node.EndPoint().Row) + 1

		// Iterate over unique lines only (keys in depths map)
		for line := range depths {
			if startLine <= line && line <= endLine {
				depths[line]++
			}
		}
		return true
	})

	return depths
}

// Analyze analyzes all files for feature flags.
// Progress is tracked via context using analyzer.WithTracker.
func (a *Analyzer) Analyze(ctx context.Context, files []string) (*Analysis, error) {
	// Filter to supported languages
	supportedFiles := a.filterSupportedFiles(files)

	// Collect all references using concurrent file processing with context
	allRefs, errs := fileproc.MapFilesWithSizeLimit(ctx, supportedFiles, a.maxFileSize, func(p *parser.Parser, path string) ([]Reference, error) {
		return a.analyzeFileWithParser(p, path)
	})

	// Flatten results
	var refs []Reference
	for _, fileRefs := range allRefs {
		refs = append(refs, fileRefs...)
	}

	// Aggregate by flag key
	analysis := a.aggregateResults(refs, supportedFiles)

	// Return processing errors if any occurred
	if errs != nil && errs.HasErrors() {
		return analysis, errs
	}

	return analysis, nil
}

// filterSupportedFiles filters to files with supported languages.
func (a *Analyzer) filterSupportedFiles(files []string) []string {
	supported := make([]string, 0, len(files))
	for _, f := range files {
		lang := parser.DetectLanguage(f)
		if LanguageToDirName(lang) != "" {
			supported = append(supported, f)
		}
	}
	return supported
}

// aggregateResults aggregates references into per-flag analysis.
func (a *Analyzer) aggregateResults(refs []Reference, files []string) *Analysis {
	// Group references by flag key
	byFlag := make(map[string][]Reference)
	for _, ref := range refs {
		byFlag[ref.FlagKey] = append(byFlag[ref.FlagKey], ref)
	}

	// Build flag analysis for each unique flag
	flags := make([]FlagAnalysis, 0, len(byFlag))

	for flagKey, flagRefs := range byFlag {
		fa := FlagAnalysis{
			FlagKey:    flagKey,
			Provider:   flagRefs[0].Provider, // Use first reference's provider
			References: flagRefs,
			Complexity: a.calculateComplexity(flagRefs),
		}

		// Calculate staleness if git is available
		if a.includeGit {
			fa.Staleness = a.calculateStaleness(flagKey, files)
		}

		// Calculate priority
		fa.Priority = CalculatePriority(fa.Staleness, fa.Complexity)

		flags = append(flags, fa)
	}

	// Sort by priority (highest first)
	sort.Slice(flags, func(i, j int) bool {
		return flags[i].Priority.Score > flags[j].Priority.Score
	})

	// Build summary
	summary := a.buildSummary(flags)

	return &Analysis{
		GeneratedAt: time.Now(),
		Flags:       flags,
		Summary:     summary,
	}
}

// calculateComplexity computes complexity metrics for a flag's references.
func (a *Analyzer) calculateComplexity(refs []Reference) Complexity {
	// File spread
	files := make(map[string]bool)
	for _, ref := range refs {
		files[ref.File] = true
	}

	// Max nesting depth
	maxNesting := 0
	for _, ref := range refs {
		if ref.NestingDepth > maxNesting {
			maxNesting = ref.NestingDepth
		}
	}

	// Coupled flags (flags that appear in the same conditional blocks)
	coupledMap := make(map[string]bool)
	for _, ref := range refs {
		for _, sibling := range ref.SiblingFlags {
			coupledMap[sibling] = true
		}
	}
	coupled := make([]string, 0, len(coupledMap))
	for f := range coupledMap {
		coupled = append(coupled, f)
	}

	return Complexity{
		FileSpread:      len(files),
		MaxNestingDepth: maxNesting,
		DecisionPoints:  len(refs),
		CoupledFlags:    coupled,
		CyclomaticDelta: len(refs), // Each flag check adds one decision point
	}
}

// calculateStaleness computes git-based staleness metrics for a flag.
func (a *Analyzer) calculateStaleness(flagKey string, files []string) *Staleness {
	if a.vcsOpener == nil || len(files) == 0 {
		return nil
	}

	// Try to open a git repo from one of the files
	var repo vcs.Repository
	for _, f := range files {
		r, err := a.vcsOpener.PlainOpenWithDetect(f)
		if err == nil {
			repo = r
			break
		}
	}

	if repo == nil {
		return nil
	}

	// Search git history for the flag key
	staleness := &Staleness{}
	now := time.Now()
	authors := make(map[string]bool)

	// Get commit iterator
	logOpts := &vcs.LogOptions{}
	iter, err := repo.Log(logOpts)
	if err != nil {
		return nil
	}

	err = iter.ForEach(func(commit vcs.Commit) error {
		msg := commit.Message()
		// Simple check: does commit message or diff mention the flag?
		if strings.Contains(msg, flagKey) {
			commitTime := commit.Author().When
			if staleness.IntroducedAt == nil || commitTime.Before(*staleness.IntroducedAt) {
				staleness.IntroducedAt = &commitTime
			}
			if staleness.LastModifiedAt == nil || commitTime.After(*staleness.LastModifiedAt) {
				staleness.LastModifiedAt = &commitTime
			}
			staleness.TotalCommits++
			authors[commit.Author().Name] = true
		}
		return nil
	})
	iter.Close()

	if err != nil || staleness.TotalCommits == 0 {
		return nil
	}

	// Calculate days since intro/modified
	if staleness.IntroducedAt != nil {
		staleness.DaysSinceIntro = int(now.Sub(*staleness.IntroducedAt).Hours() / 24)
	}
	if staleness.LastModifiedAt != nil {
		staleness.DaysSinceModified = int(now.Sub(*staleness.LastModifiedAt).Hours() / 24)
	}

	// Collect authors
	for author := range authors {
		staleness.Authors = append(staleness.Authors, author)
	}
	sort.Strings(staleness.Authors)

	// Calculate staleness score
	staleness.CalculateStalenessScore(a.expectedTTL)

	return staleness
}

// buildSummary creates the analysis summary.
func (a *Analyzer) buildSummary(flags []FlagAnalysis) Summary {
	summary := NewSummary()
	summary.TotalFlags = len(flags)

	var totalSpread int
	maxSpread := 0

	for _, f := range flags {
		summary.TotalReferences += len(f.References)
		summary.ByPriority[f.Priority.Level]++
		summary.ByProvider[f.Provider]++

		totalSpread += f.Complexity.FileSpread
		if f.Complexity.FileSpread > maxSpread {
			maxSpread = f.Complexity.FileSpread
		}

		// Track coupled flags
		for _, coupled := range f.Complexity.CoupledFlags {
			// Add to top coupled if not already tracking too many
			if len(summary.TopCoupled) < 10 {
				found := false
				for _, existing := range summary.TopCoupled {
					if existing == coupled {
						found = true
						break
					}
				}
				if !found {
					summary.TopCoupled = append(summary.TopCoupled, coupled)
				}
			}
		}
	}

	if len(flags) > 0 {
		summary.AvgFileSpread = float64(totalSpread) / float64(len(flags))
	}
	summary.MaxFileSpread = maxSpread

	return summary
}

// GetSupportedLanguages returns the list of languages with registered queries.
func (a *Analyzer) GetSupportedLanguages() []parser.Language {
	return a.registry.GetAllLanguages()
}

// GetProviders returns available providers for a language.
func (a *Analyzer) GetProviders(lang parser.Language) []string {
	return a.registry.GetProviders(lang)
}
