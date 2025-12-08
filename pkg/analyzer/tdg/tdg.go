package tdg

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/panbanda/omen/internal/fileproc"
	"github.com/panbanda/omen/pkg/analyzer"
)

// Analyzer implements TDG analysis using heuristic methods.
type Analyzer struct {
	config      Config
	maxFileSize int64
}

// Compile-time check that Analyzer implements FileAnalyzer.
var _ analyzer.FileAnalyzer[*Analysis] = (*Analyzer)(nil)

// Option is a functional option for configuring Analyzer.
type Option func(*Analyzer)

// WithConfig sets the full TDG configuration.
func WithConfig(config Config) Option {
	return func(a *Analyzer) {
		a.config = config
	}
}

// WithMaxFileSize sets the maximum file size to analyze (0 = no limit).
func WithMaxFileSize(maxSize int64) Option {
	return func(a *Analyzer) {
		a.maxFileSize = maxSize
	}
}

// New creates a new TDG analyzer.
func New(opts ...Option) *Analyzer {
	a := &Analyzer{
		config:      DefaultConfig(),
		maxFileSize: 0,
	}
	for _, opt := range opts {
		opt(a)
	}
	return a
}

// AnalyzeFile analyzes a single file and returns its TDG score.
func (a *Analyzer) AnalyzeFile(path string) (Score, error) {
	language := LanguageFromExtension(path)

	if a.maxFileSize > 0 {
		info, err := os.Stat(path)
		if err != nil {
			return Score{}, err
		}
		if info.Size() > a.maxFileSize {
			return Score{}, fmt.Errorf("file size %d exceeds maximum %d", info.Size(), a.maxFileSize)
		}
	}

	source, err := os.ReadFile(path)
	if err != nil {
		return Score{}, err
	}

	return a.AnalyzeSource(string(source), language, path)
}

// AnalyzeSource analyzes source code and returns its TDG score.
func (a *Analyzer) AnalyzeSource(source string, language Language, filePath string) (Score, error) {
	tracker := NewPenaltyTracker()

	score := NewScore()
	score.Language = language
	score.Confidence = language.Confidence()
	score.FilePath = filePath

	// Analyze each component
	score.StructuralComplexity = a.analyzeStructuralComplexity(source, tracker)
	score.SemanticComplexity = a.analyzeSemanticComplexity(source, tracker)
	score.DuplicationRatio = a.analyzeDuplication(source, tracker)
	score.CouplingScore = a.analyzeCoupling(source)
	score.DocCoverage = a.analyzeDocumentation(source, language)
	score.ConsistencyScore = a.analyzeConsistency(source)

	// Check for critical defects
	score.CriticalDefectsCount, score.HasCriticalDefects = a.detectCriticalDefects(source, language)

	// Store penalty attributions
	score.PenaltiesApplied = tracker.GetAttributions()

	// Calculate final score
	score.CalculateTotal()

	return score, nil
}

// Analyze processes the given files and returns TDG analysis results.
// Progress can be tracked by passing a context with analyzer.WithProgress.
func (a *Analyzer) Analyze(ctx context.Context, files []string) (*Analysis, error) {
	scores, errs := fileproc.ForEachFile(ctx, files, func(path string) (Score, error) {
		return a.AnalyzeFile(path)
	})

	if errs != nil && errs.HasErrors() {
		return nil, errs
	}

	analysis := AggregateProjectScore(scores)
	return &analysis, nil
}

// Compare compares two files or directories.
func (a *Analyzer) Compare(path1, path2 string) (Comparison, error) {
	info1, err := os.Stat(path1)
	if err != nil {
		return Comparison{}, err
	}

	info2, err := os.Stat(path2)
	if err != nil {
		return Comparison{}, err
	}

	var score1, score2 Score
	ctx := context.Background()

	if info1.IsDir() {
		files, err := a.discoverFiles(path1)
		if err != nil {
			return Comparison{}, err
		}
		project, err := a.Analyze(ctx, files)
		if err != nil {
			return Comparison{}, err
		}
		score1 = project.Average()
	} else {
		score1, err = a.AnalyzeFile(path1)
		if err != nil {
			return Comparison{}, err
		}
	}

	if info2.IsDir() {
		files, err := a.discoverFiles(path2)
		if err != nil {
			return Comparison{}, err
		}
		project, err := a.Analyze(ctx, files)
		if err != nil {
			return Comparison{}, err
		}
		score2 = project.Average()
	} else {
		score2, err = a.AnalyzeFile(path2)
		if err != nil {
			return Comparison{}, err
		}
	}

	return NewComparison(score1, score2), nil
}

// analyzeStructuralComplexity estimates structural complexity using heuristics.
func (a *Analyzer) analyzeStructuralComplexity(source string, tracker *PenaltyTracker) float32 {
	points := a.config.Weights.StructuralComplexity
	lines := strings.Split(source, "\n")
	cyclomatic := a.estimateCyclomaticComplexity(lines)

	if cyclomatic > a.config.Thresholds.MaxCyclomaticComplexity {
		excess := float32(cyclomatic - a.config.Thresholds.MaxCyclomaticComplexity)
		penalty := min32(excess*0.5, 15.0)

		applied := tracker.Apply(
			fmt.Sprintf("high_cyclomatic_%d", cyclomatic),
			MetricStructuralComplexity,
			penalty,
			fmt.Sprintf("High cyclomatic complexity: %d", cyclomatic),
		)
		points -= applied
	}

	return max32(points, 0)
}

// analyzeSemanticComplexity estimates semantic complexity (nesting depth).
func (a *Analyzer) analyzeSemanticComplexity(source string, tracker *PenaltyTracker) float32 {
	points := a.config.Weights.SemanticComplexity
	nestingDepth := a.estimateNestingDepth(source)

	if nestingDepth > int(a.config.Thresholds.MaxNestingDepth) {
		excess := float32(nestingDepth - int(a.config.Thresholds.MaxNestingDepth))
		penalty := min32(excess, 10.0)

		applied := tracker.Apply(
			fmt.Sprintf("deep_nesting_%d", nestingDepth),
			MetricSemanticComplexity,
			penalty,
			fmt.Sprintf("Deep nesting: %d levels", nestingDepth),
		)
		points -= applied
	}

	return max32(points, 0)
}

// analyzeDuplication estimates code duplication.
func (a *Analyzer) analyzeDuplication(source string, tracker *PenaltyTracker) float32 {
	points := a.config.Weights.Duplication
	ratio := a.estimateDuplicationRatio(source)

	if ratio > 0.1 {
		penalty := min32(ratio*20.0, 20.0)

		applied := tracker.Apply(
			fmt.Sprintf("duplication_%.2f", ratio),
			MetricDuplication,
			penalty,
			fmt.Sprintf("Code duplication: %.1f%%", ratio*100),
		)
		points -= applied
	}

	return max32(points, 0)
}

// analyzeCoupling estimates code coupling from imports.
func (a *Analyzer) analyzeCoupling(source string) float32 {
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
func (a *Analyzer) analyzeDocumentation(source string, language Language) float32 {
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
func (a *Analyzer) isDocComment(line string, language Language) bool {
	switch language {
	case LanguageRust:
		return strings.HasPrefix(line, "///") || strings.HasPrefix(line, "//!")
	case LanguagePython:
		return strings.HasPrefix(line, `"""`) || strings.HasPrefix(line, "'''")
	case LanguageJavaScript, LanguageTypeScript:
		return strings.HasPrefix(line, "/**") || strings.HasPrefix(line, "*")
	case LanguageGo:
		return strings.HasPrefix(line, "//")
	default:
		return strings.HasPrefix(line, "//") || strings.HasPrefix(line, "/*")
	}
}

// analyzeConsistency checks for consistent code style.
func (a *Analyzer) analyzeConsistency(source string) float32 {
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
func (a *Analyzer) estimateCyclomaticComplexity(lines []string) uint32 {
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
func (a *Analyzer) estimateNestingDepth(source string) int {
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

// estimateDuplicationRatio estimates the ratio of duplicated lines using hash-based counting.
func (a *Analyzer) estimateDuplicationRatio(source string) float32 {
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

	lineCounts := make(map[string]int)
	for _, line := range lines {
		if len(line) > 10 {
			lineCounts[line]++
		}
	}

	duplicates := 0
	for _, count := range lineCounts {
		if count > 1 {
			duplicates += count - 1
		}
	}

	return float32(duplicates) / float32(len(lines))
}

// detectCriticalDefects detects critical code defects.
// Returns the count but no longer auto-fails - critical defects now apply
// a penalty rather than zeroing the score.
func (a *Analyzer) detectCriticalDefects(source string, language Language) (int, bool) {
	count := 0

	// Count critical defects but don't use them for auto-fail
	// String literals that contain "panic(" etc. should not be counted
	lines := strings.Split(source, "\n")
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		// Skip comments and string literals
		if strings.HasPrefix(trimmed, "//") || strings.HasPrefix(trimmed, "/*") {
			continue
		}
		// Skip lines that are clearly string literals containing the pattern
		if strings.Contains(trimmed, `"panic(`) || strings.Contains(trimmed, `'panic(`) {
			continue
		}
		if strings.Contains(trimmed, `".unwrap()`) || strings.Contains(trimmed, `'.unwrap()`) {
			continue
		}

		switch language {
		case LanguageRust:
			// Detect .unwrap() without context (actual calls, not in strings)
			if strings.Contains(trimmed, ".unwrap()") {
				count++
			}
			// Detect panic! macros
			if strings.Contains(trimmed, "panic!") {
				count++
			}
		case LanguageGo:
			// Detect naked panics in non-test code (actual calls)
			if !strings.Contains(source, "func Test") {
				if strings.Contains(trimmed, "panic(") {
					count++
				}
			}
		}
	}

	// Return count but never auto-fail (HasCriticalDefects always false)
	// Instead, critical defects contribute to complexity penalties
	return count, false
}

// discoverFiles finds all analyzable files in a directory.
func (a *Analyzer) discoverFiles(dir string) ([]string, error) {
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
func (a *Analyzer) shouldSkipDirectory(name string) bool {
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
func (a *Analyzer) shouldAnalyzeFile(path string) bool {
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
func (a *Analyzer) Close() {
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
