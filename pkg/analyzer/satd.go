package analyzer

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/parser"
)

// SATDAnalyzer detects self-admitted technical debt markers.
type SATDAnalyzer struct {
	patterns     []satdPattern
	options      SATDOptions
	testPatterns []*regexp.Regexp
}

// SATDOptions configures SATD analysis behavior.
type SATDOptions struct {
	IncludeTests      bool // Include test files in analysis
	IncludeVendor     bool // Include vendor/third-party files
	AdjustSeverity    bool // Adjust severity based on context
	GenerateContextID bool // Generate context hash for identity tracking
}

// DefaultSATDOptions returns the default options.
func DefaultSATDOptions() SATDOptions {
	return SATDOptions{
		IncludeTests:      true,
		IncludeVendor:     false,
		AdjustSeverity:    true,
		GenerateContextID: true,
	}
}

type satdPattern struct {
	regex    *regexp.Regexp
	category models.DebtCategory
	severity models.Severity
}

// NewSATDAnalyzer creates a new SATD analyzer with default patterns.
func NewSATDAnalyzer() *SATDAnalyzer {
	return NewSATDAnalyzerWithOptions(DefaultSATDOptions())
}

// NewSATDAnalyzerWithOptions creates a SATD analyzer with custom options.
func NewSATDAnalyzerWithOptions(opts SATDOptions) *SATDAnalyzer {
	return &SATDAnalyzer{
		patterns:     defaultSATDPatterns(),
		options:      opts,
		testPatterns: defaultTestPatterns(),
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
func defaultSATDPatterns() []satdPattern {
	return []satdPattern{
		// Critical severity - Security concerns
		{regexp.MustCompile(`(?i)\b(SECURITY|VULN|VULNERABILITY|CVE|XSS)\b[:\s]*(.+)?`), models.DebtSecurity, models.SeverityCritical},
		{regexp.MustCompile(`(?i)\bUNSAFE\b[:\s]*(.+)?`), models.DebtSecurity, models.SeverityCritical},

		// High severity - Known defects
		{regexp.MustCompile(`(?i)\b(FIXME|FIX\s*ME)\b[:\s]*(.+)?`), models.DebtDefect, models.SeverityHigh},
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

	for scanner.Scan() {
		lineNum++
		line := scanner.Text()

		// Only scan comments
		if !isCommentLine(line, commentStyle) {
			continue
		}

		for _, pattern := range a.patterns {
			if matches := pattern.regex.FindStringSubmatch(line); matches != nil {
				description := strings.TrimSpace(line)
				if len(matches) > 1 && matches[1] != "" {
					description = strings.TrimSpace(matches[1])
				}

				severity := pattern.severity
				if a.options.AdjustSeverity {
					severity = a.adjustSeverity(severity, isTestFile, isSecurityContext, line)
				}

				debt := models.TechnicalDebt{
					Category:    pattern.category,
					Severity:    severity,
					File:        path,
					Line:        lineNum,
					Description: description,
					Marker:      extractMarker(matches[0]),
				}

				if a.options.GenerateContextID {
					debt.ContextHash = generateContextHash(path, lineNum, line)
				}

				debts = append(debts, debt)
				break // Only match first pattern per line
			}
		}
	}

	return debts, scanner.Err()
}

// shouldExcludeFile determines if a file should be skipped.
func (a *SATDAnalyzer) shouldExcludeFile(path string) bool {
	// Exclude test files if configured
	if !a.options.IncludeTests && a.isTestFile(path) {
		return true
	}

	// Exclude vendor/third-party files
	if !a.options.IncludeVendor && isVendorFile(path) {
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

// isVendorFile checks if a file is in a vendor directory.
func isVendorFile(path string) bool {
	vendorPatterns := []string{
		"/vendor/",
		"/node_modules/",
		"/third_party/",
		"/external/",
		"/.cargo/",
		"/site-packages/",
	}
	for _, pattern := range vendorPatterns {
		if strings.Contains(path, pattern) {
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

// adjustSeverity modifies severity based on context.
func (a *SATDAnalyzer) adjustSeverity(base models.Severity, isTest, isSecurity bool, line string) models.Severity {
	// Reduce severity for test code
	if isTest {
		return base.Reduce()
	}

	// Escalate severity in security contexts
	if isSecurity {
		return base.Escalate()
	}

	// Escalate if line mentions security-related terms
	lower := strings.ToLower(line)
	securityTerms := []string{"security", "vuln", "auth", "password", "inject", "xss", "csrf", "sql"}
	for _, term := range securityTerms {
		if strings.Contains(lower, term) {
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
func (a *SATDAnalyzer) AnalyzeProjectWithProgress(files []string, onProgress ProgressFunc) (*models.SATDAnalysis, error) {
	fileResults := ForEachFileWithProgress(files, func(path string) ([]models.TechnicalDebt, error) {
		return a.AnalyzeFile(path)
	}, onProgress)

	var allItems []models.TechnicalDebt
	for _, debts := range fileResults {
		allItems = append(allItems, debts...)
	}

	analysis := &models.SATDAnalysis{
		Items:   allItems,
		Summary: models.NewSATDSummary(),
	}

	for _, debt := range allItems {
		analysis.Summary.TotalItems++
		analysis.Summary.ByCategory[string(debt.Category)]++
		analysis.Summary.BySeverity[string(debt.Severity)]++
		analysis.Summary.ByFile[debt.File]++
	}

	return analysis, nil
}
