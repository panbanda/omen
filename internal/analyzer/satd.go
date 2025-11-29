package analyzer

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

// SATDAnalyzer detects self-admitted technical debt markers.
type SATDAnalyzer struct {
	patterns          []satdPattern
	includeTests      bool
	includeVendor     bool
	adjustSeverity    bool
	generateContextID bool
	strictMode        bool
	excludeTestBlocks bool
	maxFileSize       int64
	testPatterns      []*regexp.Regexp
}

// AstNodeType represents the type of AST context for a code location.
// Matches PMAT's AstNodeType enum for severity adjustment.
type AstNodeType int

const (
	// AstNodeRegular is a normal code location with no special context.
	AstNodeRegular AstNodeType = iota
	// AstNodeSecurityFunction is code within a security-related function.
	AstNodeSecurityFunction
	// AstNodeDataValidation is code within a data validation function.
	AstNodeDataValidation
	// AstNodeTestFunction is code within a test function.
	AstNodeTestFunction
	// AstNodeMockImplementation is code within a mock/stub implementation.
	AstNodeMockImplementation
)

// AstContext provides context about a code location for severity adjustment.
// Matches PMAT's AstContext struct.
type AstContext struct {
	NodeType              AstNodeType
	ParentFunction        string
	Complexity            uint32
	SiblingsCount         int
	NestingDepth          int
	SurroundingStatements []string
}

// SATDOption is a functional option for configuring SATDAnalyzer.
type SATDOption func(*SATDAnalyzer)

// WithSATDExcludeTests excludes test files from analysis.
// By default, test files are included.
func WithSATDExcludeTests() SATDOption {
	return func(a *SATDAnalyzer) {
		a.includeTests = false
	}
}

// WithSATDIncludeVendor includes vendor/third-party files in analysis.
// By default, vendor files are excluded.
func WithSATDIncludeVendor() SATDOption {
	return func(a *SATDAnalyzer) {
		a.includeVendor = true
	}
}

// WithSATDSkipSeverityAdjustment disables context-based severity adjustment.
// By default, severity is adjusted based on code context.
func WithSATDSkipSeverityAdjustment() SATDOption {
	return func(a *SATDAnalyzer) {
		a.adjustSeverity = false
	}
}

// WithSATDStrictMode enables strict mode, matching only explicit markers with colons.
// By default, relaxed matching is used.
func WithSATDStrictMode() SATDOption {
	return func(a *SATDAnalyzer) {
		a.strictMode = true
	}
}

// WithSATDMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithSATDMaxFileSize(maxSize int64) SATDOption {
	return func(a *SATDAnalyzer) {
		a.maxFileSize = maxSize
	}
}

// WithSATDIncludeTestBlocks includes SATD in Rust #[cfg(test)] blocks.
// By default, test blocks are excluded.
func WithSATDIncludeTestBlocks() SATDOption {
	return func(a *SATDAnalyzer) {
		a.excludeTestBlocks = false
	}
}

type satdPattern struct {
	regex    *regexp.Regexp
	category models.DebtCategory
	severity models.Severity
}

// NewSATDAnalyzer creates a new SATD analyzer with default options.
func NewSATDAnalyzer(opts ...SATDOption) *SATDAnalyzer {
	a := &SATDAnalyzer{
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
		a.patterns = strictSATDPatterns()
	} else {
		a.patterns = defaultSATDPatterns()
	}

	return a
}

// strictSATDPatterns returns patterns that only match explicit comment markers.
// This reduces false positives by requiring the format: // MARKER: description
func strictSATDPatterns() []satdPattern {
	return []satdPattern{
		// Strict mode: only matches explicit markers with colons
		{regexp.MustCompile(`//\s*TODO:\s+(.+)`), models.DebtRequirement, models.SeverityLow},
		{regexp.MustCompile(`//\s*FIXME:\s+(.+)`), models.DebtDefect, models.SeverityHigh},
		{regexp.MustCompile(`//\s*HACK:\s+(.+)`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`//\s*XXX:\s+(.+)`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`//\s*BUG:\s+(.+)`), models.DebtDefect, models.SeverityHigh},
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

// defaultSATDPatterns returns the standard SATD detection patterns.
// Severity levels match the reference implementation:
// - Critical: Security vulnerabilities
// - High: Defects (FIXME, BUG, BROKEN)
// - Medium: Design compromises (HACK, KLUDGE)
// - Low: TODOs, notes, minor enhancements
//
// Patterns exclude:
// - Bug tracking IDs like "BUG-012:" or "PMAT-BUG-001:"
// - Markdown headers like "### Security"
func defaultSATDPatterns() []satdPattern {
	return []satdPattern{
		// Critical severity - Security concerns
		// False positives (markdown headers, bug tracking IDs) are filtered by shouldSkipSATDProcessing
		{regexp.MustCompile(`(?i)\b(SECURITY|VULN|VULNERABILITY|CVE|XSS)\b[:\s]*(.+)?`), models.DebtSecurity, models.SeverityCritical},
		{regexp.MustCompile(`(?i)\bUNSAFE\b[:\s]*(.+)?`), models.DebtSecurity, models.SeverityCritical},

		// High severity - Known defects
		{regexp.MustCompile(`(?i)\b(FIXME|FIX\s*ME)\b[:\s]*(.+)?`), models.DebtDefect, models.SeverityHigh},
		// BUG pattern - false positives (bug tracking IDs) filtered by shouldSkipSATDProcessing
		{regexp.MustCompile(`(?i)\bBUG\b[:\s]*(.+)?`), models.DebtDefect, models.SeverityHigh},
		{regexp.MustCompile(`(?i)\bBROKEN\b[:\s]*(.+)?`), models.DebtDefect, models.SeverityHigh},

		// Medium severity - Design compromises
		{regexp.MustCompile(`(?i)\b(HACK|KLUDGE|SMELL|XXX)\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`(?i)\b(WORKAROUND|TEMP|TEMPORARY)\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bREFACTOR\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`(?i)\bCLEANUP\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`(?i)\btechnical\s+debt\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`(?i)\bcode\s+smell\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityMedium},
		{regexp.MustCompile(`(?i)\bperformance\s+(issue|problem)\b[:\s]*(.+)?`), models.DebtPerformance, models.SeverityMedium},
		{regexp.MustCompile(`(?i)\btest.*\b(disabled|skipped|failing)\b[:\s]*(.+)?`), models.DebtTest, models.SeverityMedium},

		// Low severity - TODOs, minor enhancements
		{regexp.MustCompile(`(?i)\bTODO\b[:\s]*(.+)?`), models.DebtRequirement, models.SeverityLow},
		{regexp.MustCompile(`(?i)\b(OPTIMIZE|SLOW)\b[:\s]*(.+)?`), models.DebtPerformance, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bNOTE\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bNB\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bIDEA\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bIMPROVE\b[:\s]*(.+)?`), models.DebtDesign, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bTEST\s*(THIS|ME)?\b[:\s]*(.+)?`), models.DebtTest, models.SeverityLow},
		{regexp.MustCompile(`(?i)\bUNTESTED\b[:\s]*(.+)?`), models.DebtTest, models.SeverityMedium},
	}
}

// shouldSkipSATDProcessing checks if a line should be excluded from SATD detection.
// This mirrors PMAT's should_skip_satd_processing function.
func shouldSkipSATDProcessing(line string) bool {
	trimmed := strings.TrimSpace(line)
	return isMarkdownHeader(trimmed) ||
		isBugTrackingID(trimmed) ||
		isFixedBugDescription(trimmed)
}

// isMarkdownHeader checks if a line is a markdown header (not a comment with SATD)
// Matches PMAT's is_markdown_header function.
func isMarkdownHeader(trimmed string) bool {
	if !strings.HasPrefix(trimmed, "#") {
		return false
	}
	// Remove leading # symbols and whitespace to get header content
	content := strings.TrimSpace(strings.TrimLeft(trimmed, "#"))

	// Check if it's a common section header (especially CHANGELOG sections)
	commonHeaders := []string{
		"Security", "Added", "Changed", "Deprecated", "Removed", "Fixed",
		"Unreleased", "Changelog", "CHANGELOG",
	}
	for _, header := range commonHeaders {
		if content == header {
			return true
		}
	}
	// Version header pattern like [1.0.0]
	if strings.HasPrefix(content, "[") {
		return true
	}
	return false
}

// isBugTrackingID checks if a line contains a bug tracking ID pattern.
// Matches PMAT's is_bug_tracking_id function.
// Patterns: BUG-123, PMAT-BUG-456, PROJECT-BUG-789
func isBugTrackingID(line string) bool {
	lower := strings.ToLower(line)

	// Pattern 1: BUG-XXX (where XXX is digits)
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

	// Pattern 2: PMAT-BUG-XXX, PROJECT-BUG-XXX (hyphen before BUG)
	if strings.Contains(lower, "-bug-") {
		return true
	}

	return false
}

// isFixedBugDescription checks if a comment describes a FIXED bug (not a current bug).
// Matches PMAT's is_fixed_bug_description function.
// Patterns: "Bug: Previously...", "CRITICAL FIX:", "BUG-064 FIX:"
func isFixedBugDescription(line string) bool {
	lower := strings.ToLower(line)

	// Pattern 1: "Bug: Previously..." - past tense description
	if strings.HasPrefix(lower, "bug:") && strings.Contains(lower, "previous") {
		return true
	}

	// Pattern 2: "CRITICAL FIX:", "BUG FIX:", "BUG-XXX FIX:"
	if strings.Contains(lower, " fix:") {
		return true
	}

	return false
}

// AddPattern adds a custom SATD detection pattern.
func (a *SATDAnalyzer) AddPattern(pattern string, category models.DebtCategory, severity models.Severity) error {
	re, err := regexp.Compile(pattern)
	if err != nil {
		return err
	}
	a.patterns = append(a.patterns, satdPattern{re, category, severity})
	return nil
}

// AnalyzeFile scans a file for SATD markers.
func (a *SATDAnalyzer) AnalyzeFile(path string) ([]models.TechnicalDebt, error) {
	// Check file size limit
	if a.maxFileSize > 0 {
		info, err := os.Stat(path)
		if err != nil {
			return nil, err
		}
		if info.Size() > a.maxFileSize {
			return nil, nil
		}
	}

	// Check exclusion rules
	if a.shouldExcludeFile(path) {
		return nil, nil
	}

	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var debts []models.TechnicalDebt
	scanner := bufio.NewScanner(file)
	lineNum := uint32(0)

	lang := parser.DetectLanguage(path)
	commentStyle := getCommentStyle(lang)
	isTestFile := a.isTestFile(path)
	isSecurityContext := a.isSecurityContext(path)

	// Initialize test block tracker for Rust files
	isRustFile := lang == parser.LangRust
	testTracker := newTestBlockTracker(isRustFile && a.excludeTestBlocks)

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		// Update test block tracking
		testTracker.updateFromLine(trimmed)

		// Skip SATD in test blocks for Rust files
		if testTracker.isInTestBlock() {
			continue
		}

		// Only scan comments
		if !isCommentLine(line, commentStyle) {
			continue
		}

		// Skip false positives (markdown headers, bug tracking IDs, fixed bug descriptions)
		if shouldSkipSATDProcessing(line) {
			continue
		}

		for _, pattern := range a.patterns {
			if matches := pattern.regex.FindStringSubmatch(line); matches != nil {
				description := strings.TrimSpace(line)
				if len(matches) > 1 && matches[1] != "" {
					description = strings.TrimSpace(matches[1])
				}

				severity := pattern.severity
				if a.adjustSeverity {
					severity = a.adjustSeverityImpl(severity, isTestFile, isSecurityContext, line)
				}

				debt := models.TechnicalDebt{
					Category:    pattern.category,
					Severity:    severity,
					File:        path,
					Line:        lineNum,
					Description: description,
					Marker:      extractMarker(matches[0]),
				}

				if a.generateContextID {
					debt.ContextHash = generateContextHash(path, lineNum, line)
				}

				debts = append(debts, debt)
				break // Only match first pattern per line
			}
		}
	}

	return debts, scanner.Err()
}

// testBlockTracker tracks #[cfg(test)] blocks in Rust files.
// SATD within test blocks can be excluded to reduce noise.
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
	// Count opening braces
	t.testBlockDepth += strings.Count(trimmed, "{")

	// Count closing braces
	closeCount := strings.Count(trimmed, "}")
	t.testBlockDepth -= closeCount

	// Exit test block when we've closed all braces
	if t.testBlockDepth <= 0 && strings.HasSuffix(trimmed, "}") {
		t.inTestBlock = false
		t.testBlockDepth = 0
	}
}

// shouldExcludeFile determines if a file should be skipped.
func (a *SATDAnalyzer) shouldExcludeFile(path string) bool {
	// Exclude test files if configured
	if !a.includeTests && a.isTestFile(path) {
		return true
	}

	// Exclude vendor/third-party files
	if !a.includeVendor && isVendorFile(path) {
		return true
	}

	// Exclude minified files
	if isMinifiedFile(path) {
		return true
	}

	return false
}

// isTestFile checks if a file is a test file.
func (a *SATDAnalyzer) isTestFile(path string) bool {
	for _, pattern := range a.testPatterns {
		if pattern.MatchString(path) {
			return true
		}
	}
	return false
}

// isVendorFile is an alias for the shared IsVendorFile function.
var isVendorFile = IsVendorFile

// isMinifiedFile checks if a file appears to be minified.
func isMinifiedFile(path string) bool {
	base := filepath.Base(path)
	return strings.Contains(base, ".min.") ||
		strings.HasSuffix(base, ".min.js") ||
		strings.HasSuffix(base, ".min.css")
}

// isSecurityContext checks if a file is in a security-sensitive context.
func (a *SATDAnalyzer) isSecurityContext(path string) bool {
	securityPatterns := []string{
		"auth",
		"security",
		"crypto",
		"password",
		"credential",
		"token",
		"session",
		"permission",
		"access",
		"sanitize",
		"validate",
		"escape",
	}
	lower := strings.ToLower(path)
	for _, pattern := range securityPatterns {
		if strings.Contains(lower, pattern) {
			return true
		}
	}
	return false
}

// adjustSeverityImpl modifies severity based on context.
// This is the internal implementation used by AnalyzeFile.
func (a *SATDAnalyzer) adjustSeverityImpl(base models.Severity, isTest, isSecurity bool, line string) models.Severity {
	// Build an AstContext from the legacy parameters
	ctx := AstContext{
		NodeType:   AstNodeRegular,
		Complexity: 1,
	}

	if isTest {
		ctx.NodeType = AstNodeTestFunction
	} else if isSecurity {
		ctx.NodeType = AstNodeSecurityFunction
	}

	// Check for security terms in content to potentially escalate
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
// Matches PMAT's adjust_severity method:
// - SecurityFunction/DataValidation: escalate severity
// - TestFunction/MockImplementation: reduce severity
// - Regular with complexity > 20: escalate severity
func (a *SATDAnalyzer) AdjustSeverityWithContext(base models.Severity, ctx *AstContext) models.Severity {
	switch ctx.NodeType {
	case AstNodeSecurityFunction, AstNodeDataValidation:
		// Critical paths escalate severity
		return base.Escalate()
	case AstNodeTestFunction, AstNodeMockImplementation:
		// Test code reduces severity
		return base.Reduce()
	case AstNodeRegular:
		// Hot paths (high complexity) escalate severity
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

// commentStyle defines comment syntax for a language.
type commentStyle struct {
	lineComments []string
	blockStart   string
	blockEnd     string
}

// getCommentStyle returns comment syntax for a language.
func getCommentStyle(lang parser.Language) commentStyle {
	switch lang {
	case parser.LangPython, parser.LangRuby, parser.LangBash:
		return commentStyle{
			lineComments: []string{"#"},
			blockStart:   `"""`,
			blockEnd:     `"""`,
		}
	case parser.LangGo, parser.LangRust, parser.LangJava, parser.LangC, parser.LangCPP,
		parser.LangCSharp, parser.LangTypeScript, parser.LangJavaScript, parser.LangTSX, parser.LangPHP:
		return commentStyle{
			lineComments: []string{"//"},
			blockStart:   "/*",
			blockEnd:     "*/",
		}
	default:
		return commentStyle{
			lineComments: []string{"//", "#"},
			blockStart:   "/*",
			blockEnd:     "*/",
		}
	}
}

// isCommentLine checks if a line is a comment.
func isCommentLine(line string, style commentStyle) bool {
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
	// Check for common comment markers that might be inside block comments
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

// AnalyzeProject scans all files in a project for SATD using parallel processing.
func (a *SATDAnalyzer) AnalyzeProject(files []string) (*models.SATDAnalysis, error) {
	return a.AnalyzeProjectWithProgress(files, nil)
}

// AnalyzeProjectWithProgress scans all files with optional progress callback.
func (a *SATDAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress fileproc.ProgressFunc) (*models.SATDAnalysis, error) {
	fileResults := fileproc.ForEachFileWithProgress(files, func(path string) ([]models.TechnicalDebt, error) {
		return a.AnalyzeFile(path)
	}, onProgress)

	var allItems []models.TechnicalDebt
	for _, debts := range fileResults {
		allItems = append(allItems, debts...)
	}

	analysis := &models.SATDAnalysis{
		Items:              allItems,
		Summary:            models.NewSATDSummary(),
		TotalFilesAnalyzed: len(files),
	}

	filesWithDebtSet := make(map[string]bool)
	for _, debt := range allItems {
		analysis.Summary.TotalItems++
		analysis.Summary.ByCategory[string(debt.Category)]++
		analysis.Summary.BySeverity[string(debt.Severity)]++
		analysis.Summary.ByFile[debt.File]++
		filesWithDebtSet[debt.File] = true
	}
	analysis.FilesWithDebt = len(filesWithDebtSet)
	analysis.Summary.FilesWithSATD = len(filesWithDebtSet)

	return analysis, nil
}
