package analyzer

import (
	"bufio"
	"os"
	"regexp"
	"strings"

	"github.com/jonathanreyes/omen-cli/pkg/models"
	"github.com/jonathanreyes/omen-cli/pkg/parser"
)

// SATDAnalyzer detects self-admitted technical debt markers.
type SATDAnalyzer struct {
	patterns []satdPattern
}

type satdPattern struct {
	regex    *regexp.Regexp
	category models.DebtCategory
	severity models.Severity
}

// NewSATDAnalyzer creates a new SATD analyzer with default patterns.
func NewSATDAnalyzer() *SATDAnalyzer {
	return &SATDAnalyzer{
		patterns: defaultSATDPatterns(),
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

				debt := models.TechnicalDebt{
					Category:    pattern.category,
					Severity:    pattern.severity,
					File:        path,
					Line:        lineNum,
					Description: description,
					Marker:      extractMarker(matches[0]),
				}
				debts = append(debts, debt)
				break // Only match first pattern per line
			}
		}
	}

	return debts, scanner.Err()
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
