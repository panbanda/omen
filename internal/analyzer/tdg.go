package analyzer

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/panbanda/omen/pkg/models"
)

// TdgAnalyzer implements TDG analysis using heuristic methods.
type TdgAnalyzer struct {
	config models.TdgConfig
}

// NewTdgAnalyzer creates a new TDG analyzer.
func NewTdgAnalyzer() *TdgAnalyzer {
	return &TdgAnalyzer{
		config: models.DefaultTdgConfig(),
	}
}

// NewTdgAnalyzerWithConfig creates a TDG analyzer with custom config.
func NewTdgAnalyzerWithConfig(config models.TdgConfig) *TdgAnalyzer {
	return &TdgAnalyzer{
		config: config,
	}
}

// AnalyzeFile analyzes a single file and returns its TDG score.
func (a *TdgAnalyzer) AnalyzeFile(path string) (models.TdgScore, error) {
	language := models.LanguageFromExtension(path)

	source, err := os.ReadFile(path)
	if err != nil {
		return models.TdgScore{}, err
	}

	return a.AnalyzeSource(string(source), language, path)
}

// AnalyzeSource analyzes source code and returns its TDG score.
func (a *TdgAnalyzer) AnalyzeSource(source string, language models.Language, filePath string) (models.TdgScore, error) {
	tracker := models.NewPenaltyTracker()

	score := models.NewTdgScore()
	score.Language = language
	score.Confidence = language.Confidence()
	score.FilePath = filePath

	// Analyze each component
	score.StructuralComplexity = a.analyzeStructuralComplexity(source, tracker)
	score.SemanticComplexity = a.analyzeSemanticComplexity(source, tracker)
	score.DuplicationRatio = a.analyzeDuplication(source, tracker)
	score.CouplingScore = a.analyzeCoupling(source, tracker)
	score.DocCoverage = a.analyzeDocumentation(source, language, tracker)
	score.ConsistencyScore = a.analyzeConsistency(source, language, tracker)

	// Check for critical defects
	score.CriticalDefectsCount, score.HasCriticalDefects = a.detectCriticalDefects(source, language)

	// Store penalty attributions
	score.PenaltiesApplied = tracker.GetAttributions()

	// Calculate final score
	score.CalculateTotal()

	return score, nil
}

// AnalyzeProject analyzes all supported files in a directory.
func (a *TdgAnalyzer) AnalyzeProject(dir string) (models.ProjectScore, error) {
	files, err := a.discoverFiles(dir)
	if err != nil {
		return models.ProjectScore{}, err
	}

	var scores []models.TdgScore
	for _, file := range files {
		score, err := a.AnalyzeFile(file)
		if err != nil {
			// Log warning but continue
			fmt.Fprintf(os.Stderr, "Warning: Failed to analyze %s: %v\n", file, err)
			continue
		}
		scores = append(scores, score)
	}

	return models.AggregateProjectScore(scores), nil
}

// Compare compares two files or directories.
func (a *TdgAnalyzer) Compare(path1, path2 string) (models.TdgComparison, error) {
	info1, err := os.Stat(path1)
	if err != nil {
		return models.TdgComparison{}, err
	}

	info2, err := os.Stat(path2)
	if err != nil {
		return models.TdgComparison{}, err
	}

	var score1, score2 models.TdgScore

	if info1.IsDir() {
		project, err := a.AnalyzeProject(path1)
		if err != nil {
			return models.TdgComparison{}, err
		}
		score1 = project.Average()
	} else {
		score1, err = a.AnalyzeFile(path1)
		if err != nil {
			return models.TdgComparison{}, err
		}
	}

	if info2.IsDir() {
		project, err := a.AnalyzeProject(path2)
		if err != nil {
			return models.TdgComparison{}, err
		}
		score2 = project.Average()
	} else {
		score2, err = a.AnalyzeFile(path2)
		if err != nil {
			return models.TdgComparison{}, err
		}
	}

	return models.NewTdgComparison(score1, score2), nil
}

// analyzeStructuralComplexity estimates structural complexity using heuristics.
func (a *TdgAnalyzer) analyzeStructuralComplexity(source string, tracker *models.PenaltyTracker) float32 {
	points := a.config.Weights.StructuralComplexity
	lines := strings.Split(source, "\n")
	cyclomatic := a.estimateCyclomaticComplexity(lines)

	if cyclomatic > a.config.Thresholds.MaxCyclomaticComplexity {
		excess := float32(cyclomatic - a.config.Thresholds.MaxCyclomaticComplexity)
		penalty := min32(excess*0.5, 15.0)

		applied := tracker.Apply(
			fmt.Sprintf("high_cyclomatic_%d", cyclomatic),
			models.MetricStructuralComplexity,
			penalty,
			fmt.Sprintf("High cyclomatic complexity: %d", cyclomatic),
		)
		points -= applied
	}

	return max32(points, 0)
}

// analyzeSemanticComplexity estimates semantic complexity (nesting depth).
func (a *TdgAnalyzer) analyzeSemanticComplexity(source string, tracker *models.PenaltyTracker) float32 {
	points := a.config.Weights.SemanticComplexity
	nestingDepth := a.estimateNestingDepth(source)

	if nestingDepth > int(a.config.Thresholds.MaxNestingDepth) {
		excess := float32(nestingDepth - int(a.config.Thresholds.MaxNestingDepth))
		penalty := min32(excess, 10.0)

		applied := tracker.Apply(
			fmt.Sprintf("deep_nesting_%d", nestingDepth),
			models.MetricSemanticComplexity,
			penalty,
			fmt.Sprintf("Deep nesting: %d levels", nestingDepth),
		)
		points -= applied
	}

	return max32(points, 0)
}

// analyzeDuplication estimates code duplication.
func (a *TdgAnalyzer) analyzeDuplication(source string, tracker *models.PenaltyTracker) float32 {
	points := a.config.Weights.Duplication
	ratio := a.estimateDuplicationRatio(source)

	if ratio > 0.1 {
		penalty := min32(ratio*20.0, 20.0)

		applied := tracker.Apply(
			fmt.Sprintf("duplication_%.2f", ratio),
			models.MetricDuplication,
			penalty,
			fmt.Sprintf("Code duplication: %.1f%%", ratio*100),
		)
		points -= applied
	}

	return max32(points, 0)
}

// analyzeCoupling estimates code coupling from imports.
func (a *TdgAnalyzer) analyzeCoupling(source string, _ *models.PenaltyTracker) float32 {
	importCount := 0
	for _, line := range strings.Split(source, "\n") {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "use ") ||
			strings.HasPrefix(trimmed, "import ") ||
			strings.HasPrefix(trimmed, "from ") ||
			strings.HasPrefix(trimmed, "#include ") {
			importCount++
		}
	}

	baseScore := a.config.Weights.Coupling
	if importCount > 20 {
		penalty := min32(float32(importCount-20)*0.2, 10.0)
		return max32(baseScore-penalty, 0)
	}
	return baseScore
}

// analyzeDocumentation estimates documentation coverage.
func (a *TdgAnalyzer) analyzeDocumentation(source string, language models.Language, _ *models.PenaltyTracker) float32 {
	lines := strings.Split(source, "\n")
	totalLines := len(lines)
	if totalLines == 0 {
		return a.config.Weights.Documentation
	}

	docLines := 0
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if a.isDocComment(trimmed, language) {
			docLines++
		}
	}

	coverage := float32(docLines) / float32(totalLines)
	return min32(coverage*a.config.Weights.Documentation, a.config.Weights.Documentation)
}

// isDocComment checks if a line is a documentation comment.
func (a *TdgAnalyzer) isDocComment(line string, language models.Language) bool {
	switch language {
	case models.LanguageRust:
		return strings.HasPrefix(line, "///") || strings.HasPrefix(line, "//!")
	case models.LanguagePython:
		return strings.HasPrefix(line, `"""`) || strings.HasPrefix(line, "'''")
	case models.LanguageJavaScript, models.LanguageTypeScript:
		return strings.HasPrefix(line, "/**") || strings.HasPrefix(line, "*")
	case models.LanguageGo:
		return strings.HasPrefix(line, "//")
	default:
		return strings.HasPrefix(line, "//") || strings.HasPrefix(line, "/*")
	}
}

// analyzeConsistency checks for consistent code style.
func (a *TdgAnalyzer) analyzeConsistency(source string, _ models.Language, _ *models.PenaltyTracker) float32 {
	lines := strings.Split(source, "\n")
	if len(lines) == 0 {
		return a.config.Weights.Consistency
	}

	tabCount := 0
	spaceCount := 0

	for _, line := range lines {
		if strings.HasPrefix(line, "\t") {
			tabCount++
		} else if strings.HasPrefix(line, "    ") || strings.HasPrefix(line, "  ") {
			spaceCount++
		}
	}

	totalIndented := tabCount + spaceCount
	if totalIndented == 0 {
		return a.config.Weights.Consistency
	}

	var consistency float32
	if tabCount > spaceCount {
		consistency = float32(tabCount) / float32(totalIndented)
	} else {
		consistency = float32(spaceCount) / float32(totalIndented)
	}

	return consistency * a.config.Weights.Consistency
}

// estimateCyclomaticComplexity estimates cyclomatic complexity from source.
func (a *TdgAnalyzer) estimateCyclomaticComplexity(lines []string) uint32 {
	complexity := uint32(1) // Base complexity

	for _, line := range lines {
		trimmed := strings.TrimSpace(line)

		// Control flow statements
		if strings.HasPrefix(trimmed, "if ") || strings.Contains(trimmed, " if ") {
			complexity++
		}
		if strings.HasPrefix(trimmed, "for ") || strings.Contains(trimmed, " for ") {
			complexity++
		}
		if strings.HasPrefix(trimmed, "while ") || strings.Contains(trimmed, " while ") {
			complexity++
		}
		if strings.HasPrefix(trimmed, "match ") || strings.Contains(trimmed, " match ") {
			complexity++
		}
		if strings.HasPrefix(trimmed, "switch ") || strings.Contains(trimmed, " switch ") {
			complexity++
		}
		if strings.HasPrefix(trimmed, "case ") {
			complexity++
		}
		if strings.HasPrefix(trimmed, "select ") || strings.Contains(trimmed, " select ") {
			complexity++
		}

		// Logical operators add to complexity
		complexity += uint32(strings.Count(trimmed, " && "))
		complexity += uint32(strings.Count(trimmed, " || "))
	}

	return complexity
}

// estimateNestingDepth estimates maximum nesting depth.
func (a *TdgAnalyzer) estimateNestingDepth(source string) int {
	maxDepth := 0
	currentDepth := 0

	for _, line := range strings.Split(source, "\n") {
		trimmed := strings.TrimSpace(line)
		currentDepth += strings.Count(trimmed, "{")
		if currentDepth > maxDepth {
			maxDepth = currentDepth
		}
		currentDepth -= strings.Count(trimmed, "}")
		if currentDepth < 0 {
			currentDepth = 0
		}
	}

	return maxDepth
}

// estimateDuplicationRatio estimates the ratio of duplicated lines.
func (a *TdgAnalyzer) estimateDuplicationRatio(source string) float32 {
	lines := []string{}
	for _, line := range strings.Split(source, "\n") {
		trimmed := strings.TrimSpace(line)
		if trimmed != "" && !strings.HasPrefix(trimmed, "//") && !strings.HasPrefix(trimmed, "/*") {
			lines = append(lines, trimmed)
		}
	}

	if len(lines) < 3 {
		return 0
	}

	duplicates := 0
	for i := 0; i < len(lines); i++ {
		for j := i + 1; j < len(lines); j++ {
			if lines[i] == lines[j] && len(lines[i]) > 10 {
				duplicates++
			}
		}
	}

	return float32(duplicates) / float32(len(lines))
}

// detectCriticalDefects detects critical code defects.
func (a *TdgAnalyzer) detectCriticalDefects(source string, language models.Language) (int, bool) {
	count := 0

	switch language {
	case models.LanguageRust:
		// Detect .unwrap() without context
		count += strings.Count(source, ".unwrap()")
		// Detect panic! macros
		count += strings.Count(source, "panic!")
	case models.LanguageGo:
		// Detect naked panics in non-test code
		if !strings.Contains(source, "func Test") {
			count += strings.Count(source, "panic(")
		}
	}

	return count, count > 0
}

// discoverFiles finds all analyzable files in a directory.
func (a *TdgAnalyzer) discoverFiles(dir string) ([]string, error) {
	var files []string

	err := filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}

		if info.IsDir() {
			if a.shouldSkipDirectory(info.Name()) {
				return filepath.SkipDir
			}
			return nil
		}

		if a.shouldAnalyzeFile(path) {
			files = append(files, path)
		}

		return nil
	})

	return files, err
}

// shouldSkipDirectory returns true for directories that should be skipped.
func (a *TdgAnalyzer) shouldSkipDirectory(name string) bool {
	skipDirs := map[string]bool{
		"node_modules":  true,
		"target":        true,
		"build":         true,
		"dist":          true,
		".git":          true,
		"__pycache__":   true,
		".pytest_cache": true,
		"venv":          true,
		".venv":         true,
		"vendor":        true,
		".idea":         true,
		".vscode":       true,
	}
	return skipDirs[name]
}

// shouldAnalyzeFile returns true for files that should be analyzed.
func (a *TdgAnalyzer) shouldAnalyzeFile(path string) bool {
	ext := strings.ToLower(filepath.Ext(path))
	analyzableExts := map[string]bool{
		".rs":    true,
		".py":    true,
		".js":    true,
		".ts":    true,
		".jsx":   true,
		".tsx":   true,
		".go":    true,
		".java":  true,
		".c":     true,
		".h":     true,
		".cpp":   true,
		".cc":    true,
		".cxx":   true,
		".hpp":   true,
		".rb":    true,
		".swift": true,
		".kt":    true,
		".kts":   true,
	}
	return analyzableExts[ext]
}

// Close releases any resources (implements interface compatibility).
func (a *TdgAnalyzer) Close() {
	// No resources to release
}

func min32(a, b float32) float32 {
	if a < b {
		return a
	}
	return b
}

func max32(a, b float32) float32 {
	if a > b {
		return a
	}
	return b
}
