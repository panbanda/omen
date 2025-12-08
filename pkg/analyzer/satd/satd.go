package satd

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/parser"
)

// Analyzer detects self-admitted technical debt markers.
type Analyzer struct {
	patterns          []pattern
	includeTests      bool
	includeVendor     bool
	adjustSeverity    bool
	generateContextID bool
	strictMode        bool
	excludeTestBlocks bool
	maxFileSize       int64
	testPatterns      []*regexp.Regexp
}

// Compile-time check that Analyzer implements SourceFileAnalyzer.
var _ analyzer.SourceFileAnalyzer[*Analysis] = (*Analyzer)(nil)

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithSkipTests excludes test files from analysis.
// By default, test files are included.
func WithSkipTests() Option {
	return func(a *Analyzer) {
		a.includeTests = false
	}
}

// WithIncludeVendor includes vendor/third-party files in analysis.
// By default, vendor files are excluded.
func WithIncludeVendor() Option {
	return func(a *Analyzer) {
		a.includeVendor = true
	}
}

// WithSkipSeverityAdjustment disables context-based severity adjustment.
// By default, severity is adjusted based on code context.
func WithSkipSeverityAdjustment() Option {
	return func(a *Analyzer) {
		a.adjustSeverity = false
	}
}

// WithStrictMode enables strict mode, matching only explicit markers with colons.
// By default, relaxed matching is used.
func WithStrictMode() Option {
	return func(a *Analyzer) {
		a.strictMode = true
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// WithIncludeTestBlocks includes SATD in Rust #[cfg(test)] blocks.
// By default, test blocks are excluded.
func WithIncludeTestBlocks() Option {
	return func(a *Analyzer) {
		a.excludeTestBlocks = false
	}
}

type pattern struct {
	regex    *regexp.Regexp
	category Category
	severity Severity
}

// New creates a new SATD analyzer with default options.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		includeTests:      true,
		includeVendor:     false,
		adjustSeverity:    true,
		generateContextID: true,
		strictMode:        false,
		excludeTestBlocks: true,
		maxFileSize:       0,
		testPatterns:      defaultTestPatterns(),
	}

	for _, opt := range opts {
		opt(a)
	}

	if a.strictMode {
		a.patterns = strictPatterns()
	} else {
		a.patterns = defaultPatterns()
	}

	return a
}

// strictPatterns returns patterns that only match explicit comment markers.
// This reduces false positives by requiring the format: // MARKER: description
func strictPatterns() []pattern {
	return []pattern{
		{regexp.MustCompile(`//\s*TODO:\s+(.+)`), CategoryRequirement, SeverityLow},
		{regexp.MustCompile(`//\s*FIXME:\s+(.+)`), CategoryDefect, SeverityHigh},
		{regexp.MustCompile(`//\s*HACK:\s+(.+)`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`//\s*XXX:\s+(.+)`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`//\s*BUG:\s+(.+)`), CategoryDefect, SeverityHigh},
	}
}

// defaultTestPatterns returns patterns for detecting test files.
func defaultTestPatterns() []*regexp.Regexp {
	return []*regexp.Regexp{
		regexp.MustCompile(`_test\.go$`),
		regexp.MustCompile(`test_.*\.py$`),
		regexp.MustCompile(`.*_test\.py$`),
		regexp.MustCompile(`.*\.test\.[jt]sx?$`),
		regexp.MustCompile(`.*\.spec\.[jt]sx?$`),
		regexp.MustCompile(`__tests__/`),
		regexp.MustCompile(`tests?/`),
		regexp.MustCompile(`spec/`),
		regexp.MustCompile(`Test\.java$`),
		regexp.MustCompile(`_test\.rs$`),
		regexp.MustCompile(`_spec\.rb$`),
	}
}

// defaultPatterns returns the standard SATD detection patterns.
// Severity levels:
// - Critical: Security vulnerabilities
// - High: Defects (FIXME, BUG, BROKEN)
// - Medium: Design compromises (HACK, KLUDGE)
// - Low: TODOs, notes, minor enhancements
func defaultPatterns() []pattern {
	return []pattern{
		// Critical severity - Security concerns
		{regexp.MustCompile(`(?i)\b(SECURITY|VULN|VULNERABILITY|CVE|XSS)\b[:\s]*(.+)?`), CategorySecurity, SeverityCritical},
		{regexp.MustCompile(`(?i)\bUNSAFE\b[:\s]*(.+)?`), CategorySecurity, SeverityCritical},

		// High severity - Known defects
		{regexp.MustCompile(`(?i)\b(FIXME|FIX\s*ME)\b[:\s]*(.+)?`), CategoryDefect, SeverityHigh},
		{regexp.MustCompile(`(?i)\bBUG\b[:\s]*(.+)?`), CategoryDefect, SeverityHigh},
		{regexp.MustCompile(`(?i)\bBROKEN\b[:\s]*(.+)?`), CategoryDefect, SeverityHigh},

		// Medium severity - Design compromises
		{regexp.MustCompile(`(?i)\b(HACK|KLUDGE|SMELL|XXX)\b[:\s]*(.+)?`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`(?i)\b(WORKAROUND|TEMP|TEMPORARY)\b[:\s]*(.+)?`), CategoryDesign, SeverityLow},
		{regexp.MustCompile(`(?i)\bREFACTOR\b[:\s]*(.+)?`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`(?i)\bCLEANUP\b[:\s]*(.+)?`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`(?i)\btechnical\s+debt\b[:\s]*(.+)?`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`(?i)\bcode\s+smell\b[:\s]*(.+)?`), CategoryDesign, SeverityMedium},
		{regexp.MustCompile(`(?i)\bperformance\s+(issue|problem)\b[:\s]*(.+)?`), CategoryPerformance, SeverityMedium},
		{regexp.MustCompile(`(?i)\btest.*\b(disabled|skipped|failing)\b[:\s]*(.+)?`), CategoryTest, SeverityMedium},

		// Low severity - TODOs, minor enhancements
		{regexp.MustCompile(`(?i)\bTODO\b[:\s]*(.+)?`), CategoryRequirement, SeverityLow},
		{regexp.MustCompile(`(?i)\b(OPTIMIZE|SLOW)\b[:\s]*(.+)?`), CategoryPerformance, SeverityLow},
		{regexp.MustCompile(`(?i)\bNOTE\b[:\s]*(.+)?`), CategoryDesign, SeverityLow},
		{regexp.MustCompile(`(?i)\bNB\b[:\s]*(.+)?`), CategoryDesign, SeverityLow},
		{regexp.MustCompile(`(?i)\bIDEA\b[:\s]*(.+)?`), CategoryDesign, SeverityLow},
		{regexp.MustCompile(`(?i)\bIMPROVE\b[:\s]*(.+)?`), CategoryDesign, SeverityLow},
		{regexp.MustCompile(`(?i)\bTEST\s*(THIS|ME)?\b[:\s]*(.+)?`), CategoryTest, SeverityLow},
		{regexp.MustCompile(`(?i)\bUNTESTED\b[:\s]*(.+)?`), CategoryTest, SeverityMedium},
	}
}

// shouldSkipProcessing checks if a line should be excluded from SATD detection.
func shouldSkipProcessing(line string) bool {
	trimmed := strings.TrimSpace(line)
	return isMarkdownHeader(trimmed) ||
		isBugTrackingID(trimmed) ||
		isFixedBugDescription(trimmed) ||
		hasIgnoreDirective(line)
}

// hasIgnoreDirective checks if a line contains an omen:ignore directive.
// Supports: omen:ignore, omen:ignore-line, omen:ignore-satd
func hasIgnoreDirective(line string) bool {
	lower := strings.ToLower(line)
	return strings.Contains(lower, "omen:ignore")
}

// isMarkdownHeader checks if a line is a markdown header.
func isMarkdownHeader(trimmed string) bool {
	if !strings.HasPrefix(trimmed, "#") {
		return false
	}
	content := strings.TrimSpace(strings.TrimLeft(trimmed, "#"))

	commonHeaders := []string{
		"Security", "Added", "Changed", "Deprecated", "Removed", "Fixed",
		"Unreleased", "Changelog", "CHANGELOG",
	}
	for _, header := range commonHeaders {
		if content == header {
			return true
		}
	}
	return strings.HasPrefix(content, "[")
}

// isBugTrackingID checks if a line contains a bug tracking ID pattern.
func isBugTrackingID(line string) bool {
	lower := strings.ToLower(line)

	if strings.Contains(lower, "bug-") {
		idx := strings.Index(lower, "bug-")
		if idx >= 0 && idx+4 < len(line) {
			afterDash := line[idx+4:]
			digitCount := 0
			for _, c := range afterDash {
				if c >= '0' && c <= '9' {
					digitCount++
				} else {
					break
				}
			}
			if digitCount >= 1 {
				return true
			}
		}
	}

	if strings.Contains(lower, "-bug-") {
		return true
	}

	return false
}

// isFixedBugDescription checks if a comment describes a FIXED bug.
func isFixedBugDescription(line string) bool {
	lower := strings.ToLower(line)

	if strings.HasPrefix(lower, "bug:") && strings.Contains(lower, "previous") {
		return true
	}

	if strings.Contains(lower, " fix:") {
		return true
	}

	return false
}

// AddPattern adds a custom SATD detection pattern.
func (a *Analyzer) AddPattern(pat string, category Category, severity Severity) error {
	re, err := regexp.Compile(pat)
	if err != nil {
		return err
	}
	a.patterns = append(a.patterns, pattern{re, category, severity})
	return nil
}

// AnalyzeFile scans a file for SATD markers.
func (a *Analyzer) AnalyzeFile(path string) ([]Item, error) {
	if a.maxFileSize > 0 {
		info, err := os.Stat(path)
		if err != nil {
			return nil, err
		}
		if info.Size() > a.maxFileSize {
			return nil, nil
		}
	}

	if a.shouldExcludeFile(path) {
		return nil, nil
	}

	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var items []Item
	scanner := bufio.NewScanner(file)
	lineNum := uint32(0)

	lang := parser.DetectLanguage(path)
	commentStyle := getCommentStyle(lang)
	isTestFile := a.isTestFile(path)
	isSecurityContext := a.isSecurityContext(path)

	isRustFile := lang == parser.LangRust
	testTracker := newTestBlockTracker(isRustFile && a.excludeTestBlocks)

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		testTracker.updateFromLine(trimmed)

		if testTracker.isInTestBlock() {
			continue
		}

		if !isCommentLine(line, commentStyle) {
			continue
		}

		if shouldSkipProcessing(line) {
			continue
		}

		for _, pat := range a.patterns {
			if matches := pat.regex.FindStringSubmatch(line); matches != nil {
				description := strings.TrimSpace(line)
				if len(matches) > 1 && matches[1] != "" {
					description = strings.TrimSpace(matches[1])
				}

				severity := pat.severity
				if a.adjustSeverity {
					severity = a.adjustSeverityImpl(severity, isTestFile, isSecurityContext, line)
				}

				item := Item{
					Category:    pat.category,
					Severity:    severity,
					File:        path,
					Line:        lineNum,
					Description: description,
					Marker:      extractMarker(matches[0]),
				}

				if a.generateContextID {
					item.ContextHash = generateContextHash(path, lineNum, line)
				}

				items = append(items, item)
				break
			}
		}
	}

	return items, scanner.Err()
}

// testBlockTracker tracks #[cfg(test)] blocks in Rust files.
type testBlockTracker struct {
	enabled        bool
	inTestBlock    bool
	testBlockDepth int
}

func newTestBlockTracker(enabled bool) *testBlockTracker {
	return &testBlockTracker{enabled: enabled}
}

func (t *testBlockTracker) updateFromLine(trimmed string) {
	if !t.enabled {
		return
	}

	if t.isTestBlockStart(trimmed) {
		t.startTestBlock()
	} else if t.inTestBlock {
		t.updateTestBlockDepth(trimmed)
	}
}

func (t *testBlockTracker) isInTestBlock() bool {
	return t.inTestBlock
}

func (t *testBlockTracker) isTestBlockStart(trimmed string) bool {
	return strings.HasPrefix(trimmed, "#[cfg(test)]")
}

func (t *testBlockTracker) startTestBlock() {
	t.inTestBlock = true
	t.testBlockDepth = 0
}

func (t *testBlockTracker) updateTestBlockDepth(trimmed string) {
	t.testBlockDepth += strings.Count(trimmed, "{")

	closeCount := strings.Count(trimmed, "}")
	t.testBlockDepth -= closeCount

	if t.testBlockDepth <= 0 && strings.HasSuffix(trimmed, "}") {
		t.inTestBlock = false
		t.testBlockDepth = 0
	}
}

// shouldExcludeFile determines if a file should be skipped.
func (a *Analyzer) shouldExcludeFile(path string) bool {
	if !a.includeTests && a.isTestFile(path) {
		return true
	}

	if !a.includeVendor && isVendorFile(path) {
		return true
	}

	if isMinifiedFile(path) {
		return true
	}

	return false
}

// isTestFile checks if a file is a test file.
func (a *Analyzer) isTestFile(path string) bool {
	for _, pat := range a.testPatterns {
		if pat.MatchString(path) {
			return true
		}
	}
	return false
}

// isVendorFile checks if a file is in a vendor directory.
func isVendorFile(path string) bool {
	parts := strings.Split(filepath.ToSlash(path), "/")
	for _, part := range parts {
		switch part {
		case "vendor", "node_modules", "third_party", "external", "deps":
			return true
		}
	}
	return false
}

// isMinifiedFile checks if a file appears to be minified.
func isMinifiedFile(path string) bool {
	base := filepath.Base(path)
	return strings.Contains(base, ".min.") ||
		strings.HasSuffix(base, ".min.js") ||
		strings.HasSuffix(base, ".min.css")
}

// isSecurityContext checks if a file is in a security-sensitive context.
func (a *Analyzer) isSecurityContext(path string) bool {
	securityPatterns := []string{
		"auth", "security", "crypto", "password", "credential",
		"token", "session", "permission", "access", "sanitize",
		"validate", "escape",
	}
	lower := strings.ToLower(path)
	for _, pat := range securityPatterns {
		if strings.Contains(lower, pat) {
			return true
		}
	}
	return false
}

// adjustSeverityImpl modifies severity based on context.
func (a *Analyzer) adjustSeverityImpl(base Severity, isTest, isSecurity bool, line string) Severity {
	ctx := AstContext{
		NodeType:   AstNodeRegular,
		Complexity: 1,
	}

	if isTest {
		ctx.NodeType = AstNodeTestFunction
	} else if isSecurity {
		ctx.NodeType = AstNodeSecurityFunction
	}

	lower := strings.ToLower(line)
	securityTerms := []string{"security", "vuln", "auth", "password", "inject", "xss", "csrf", "sql"}
	for _, term := range securityTerms {
		if strings.Contains(lower, term) {
			ctx.NodeType = AstNodeSecurityFunction
			break
		}
	}

	return a.AdjustSeverityWithContext(base, &ctx)
}

// AdjustSeverityWithContext modifies severity based on AST context.
// - SecurityFunction/DataValidation: escalate severity
// - TestFunction/MockImplementation: reduce severity
// - Regular with complexity > 20: escalate severity
func (a *Analyzer) AdjustSeverityWithContext(base Severity, ctx *AstContext) Severity {
	switch ctx.NodeType {
	case AstNodeSecurityFunction, AstNodeDataValidation:
		return base.Escalate()
	case AstNodeTestFunction, AstNodeMockImplementation:
		return base.Reduce()
	case AstNodeRegular:
		if ctx.Complexity > 20 {
			return base.Escalate()
		}
	}
	return base
}

// generateContextHash creates a stable identity hash for a debt item.
func generateContextHash(path string, line uint32, content string) string {
	h := sha256.New()
	h.Write([]byte(path))
	h.Write([]byte{byte(line >> 24), byte(line >> 16), byte(line >> 8), byte(line)})
	h.Write([]byte(strings.TrimSpace(content)))
	return hex.EncodeToString(h.Sum(nil)[:16])
}

// commentStyleInfo defines comment syntax for a language.
type commentStyleInfo struct {
	lineComments []string
	blockStart   string
	blockEnd     string
}

// getCommentStyle returns comment syntax for a language.
func getCommentStyle(lang parser.Language) commentStyleInfo {
	switch lang {
	case parser.LangPython, parser.LangRuby, parser.LangBash:
		return commentStyleInfo{
			lineComments: []string{"#"},
			blockStart:   `"""`,
			blockEnd:     `"""`,
		}
	case parser.LangGo, parser.LangRust, parser.LangJava, parser.LangC, parser.LangCPP,
		parser.LangCSharp, parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX, parser.LangPHP:
		return commentStyleInfo{
			lineComments: []string{"//"},
			blockStart:   "/*",
			blockEnd:     "*/",
		}
	default:
		return commentStyleInfo{
			lineComments: []string{"//", "#"},
			blockStart:   "/*",
			blockEnd:     "*/",
		}
	}
}

// isCommentLine checks if a line is a comment.
func isCommentLine(line string, style commentStyleInfo) bool {
	trimmed := strings.TrimSpace(line)
	for _, prefix := range style.lineComments {
		if strings.HasPrefix(trimmed, prefix) {
			return true
		}
	}
	if style.blockStart != "" {
		if strings.Contains(trimmed, style.blockStart) || strings.Contains(trimmed, style.blockEnd) {
			return true
		}
	}
	if strings.Contains(trimmed, "*") && (strings.HasPrefix(trimmed, "*") || strings.HasPrefix(trimmed, "/*")) {
		return true
	}
	return false
}

// extractMarker extracts the SATD keyword from a match.
func extractMarker(match string) string {
	markers := []string{"TODO", "FIXME", "HACK", "BUG", "XXX", "NOTE", "OPTIMIZE",
		"REFACTOR", "CLEANUP", "TEMP", "WORKAROUND", "SECURITY", "TEST"}
	upper := strings.ToUpper(match)
	for _, m := range markers {
		if strings.Contains(upper, m) {
			return m
		}
	}
	return "UNKNOWN"
}

// Close releases analyzer resources.
func (a *Analyzer) Close() {
}

// ContentSource is an alias for analyzer.ContentSource.
type ContentSource = analyzer.ContentSource

// AnalyzeFileFromSource scans content for SATD markers.
func (a *Analyzer) AnalyzeFileFromSource(path string, content []byte) ([]Item, error) {
	if a.maxFileSize > 0 && int64(len(content)) > a.maxFileSize {
		return nil, nil
	}

	if a.shouldExcludeFile(path) {
		return nil, nil
	}

	var items []Item
	scanner := bufio.NewScanner(bytes.NewReader(content))
	lineNum := uint32(0)

	lang := parser.DetectLanguage(path)
	commentStyle := getCommentStyle(lang)
	isTestFile := a.isTestFile(path)
	isSecurityContext := a.isSecurityContext(path)

	isRustFile := lang == parser.LangRust
	testTracker := newTestBlockTracker(isRustFile && a.excludeTestBlocks)

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		testTracker.updateFromLine(trimmed)

		if testTracker.isInTestBlock() {
			continue
		}

		if !isCommentLine(line, commentStyle) {
			continue
		}

		if shouldSkipProcessing(line) {
			continue
		}

		for _, pat := range a.patterns {
			if matches := pat.regex.FindStringSubmatch(line); matches != nil {
				description := strings.TrimSpace(line)
				if len(matches) > 1 && matches[1] != "" {
					description = strings.TrimSpace(matches[1])
				}

				severity := pat.severity
				if a.adjustSeverity {
					severity = a.adjustSeverityImpl(severity, isTestFile, isSecurityContext, line)
				}

				item := Item{
					Category:    pat.category,
					Severity:    severity,
					File:        path,
					Line:        lineNum,
					Description: description,
					Marker:      extractMarker(matches[0]),
				}

				if a.generateContextID {
					item.ContextHash = generateContextHash(path, lineNum, line)
				}

				items = append(items, item)
				break
			}
		}
	}

	return items, scanner.Err()
}

// Analyze scans all files from a ContentSource for SATD.
func (a *Analyzer) Analyze(ctx context.Context, files []string, src ContentSource) (*Analysis, error) {
	var allItems []Item
	filesAnalyzed := 0

	// Get progress tracker from context
	tracker := analyzer.TrackerFromContext(ctx)
	if tracker != nil {
		tracker.Add(len(files))
	}

	for _, path := range files {
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		default:
		}

		if tracker != nil {
			tracker.Tick(path)
		}

		content, err := src.Read(path)
		if err != nil {
			continue
		}

		items, err := a.AnalyzeFileFromSource(path, content)
		if err != nil {
			continue
		}

		allItems = append(allItems, items...)
		filesAnalyzed++
	}

	analysis := &Analysis{
		Items:              allItems,
		Summary:            NewSummary(),
		TotalFilesAnalyzed: filesAnalyzed,
	}

	filesWithDebtSet := make(map[string]bool)
	for _, item := range allItems {
		analysis.Summary.TotalItems++
		analysis.Summary.ByCategory[string(item.Category)]++
		analysis.Summary.BySeverity[string(item.Severity)]++
		analysis.Summary.ByFile[item.File]++
		filesWithDebtSet[item.File] = true
	}
	analysis.FilesWithDebt = len(filesWithDebtSet)
	analysis.Summary.FilesWithSATD = len(filesWithDebtSet)

	// Sort items by severity (critical first, then high, medium, low)
	sort.Slice(analysis.Items, func(i, j int) bool {
		return analysis.Items[i].Severity.Weight() > analysis.Items[j].Severity.Weight()
	})

	return analysis, nil
}
