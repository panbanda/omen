package models

import (
	"path/filepath"
	"strings"
)

// Grade represents a letter grade from A+ to F (PMAT-compatible).
// Higher grades indicate better code quality.
type Grade string

const (
	GradeAPlus  Grade = "A+"
	GradeA      Grade = "A"
	GradeAMinus Grade = "A-"
	GradeBPlus  Grade = "B+"
	GradeB      Grade = "B"
	GradeBMinus Grade = "B-"
	GradeCPlus  Grade = "C+"
	GradeC      Grade = "C"
	GradeCMinus Grade = "C-"
	GradeD      Grade = "D"
	GradeF      Grade = "F"
)

// GradeFromScore converts a 0-100 score to a letter grade.
func GradeFromScore(score float32) Grade {
	switch {
	case score >= 95.0:
		return GradeAPlus
	case score >= 90.0:
		return GradeA
	case score >= 85.0:
		return GradeAMinus
	case score >= 80.0:
		return GradeBPlus
	case score >= 75.0:
		return GradeB
	case score >= 70.0:
		return GradeBMinus
	case score >= 65.0:
		return GradeCPlus
	case score >= 60.0:
		return GradeC
	case score >= 55.0:
		return GradeCMinus
	case score >= 50.0:
		return GradeD
	default:
		return GradeF
	}
}

// MetricCategory represents a category of TDG metrics.
type MetricCategory string

const (
	MetricStructuralComplexity MetricCategory = "structural_complexity"
	MetricSemanticComplexity   MetricCategory = "semantic_complexity"
	MetricDuplication          MetricCategory = "duplication"
	MetricCoupling             MetricCategory = "coupling"
	MetricDocumentation        MetricCategory = "documentation"
	MetricConsistency          MetricCategory = "consistency"
)

// Language represents the detected programming language.
type Language string

const (
	LanguageUnknown    Language = "unknown"
	LanguageRust       Language = "rust"
	LanguageGo         Language = "go"
	LanguagePython     Language = "python"
	LanguageJavaScript Language = "javascript"
	LanguageTypeScript Language = "typescript"
	LanguageJava       Language = "java"
	LanguageC          Language = "c"
	LanguageCpp        Language = "cpp"
	LanguageCSharp     Language = "csharp"
	LanguageRuby       Language = "ruby"
	LanguagePHP        Language = "php"
	LanguageSwift      Language = "swift"
	LanguageKotlin     Language = "kotlin"
)

// LanguageFromExtension detects the language from a file extension.
func LanguageFromExtension(path string) Language {
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".rs":
		return LanguageRust
	case ".go":
		return LanguageGo
	case ".py":
		return LanguagePython
	case ".js":
		return LanguageJavaScript
	case ".ts":
		return LanguageTypeScript
	case ".jsx", ".tsx":
		return LanguageTypeScript
	case ".java":
		return LanguageJava
	case ".c", ".h":
		return LanguageC
	case ".cpp", ".cc", ".cxx", ".hpp":
		return LanguageCpp
	case ".cs":
		return LanguageCSharp
	case ".rb":
		return LanguageRuby
	case ".php":
		return LanguagePHP
	case ".swift":
		return LanguageSwift
	case ".kt", ".kts":
		return LanguageKotlin
	default:
		return LanguageUnknown
	}
}

// Confidence returns the detection confidence for the language.
func (l Language) Confidence() float32 {
	if l == LanguageUnknown {
		return 0.5
	}
	return 0.95
}

// PenaltyAttribution tracks where a penalty was applied.
type PenaltyAttribution struct {
	SourceMetric MetricCategory   `json:"source_metric"`
	Amount       float32          `json:"amount"`
	AppliedTo    []MetricCategory `json:"applied_to"`
	Issue        string           `json:"issue"`
}

// PenaltyTracker tracks penalties applied during analysis.
type PenaltyTracker struct {
	applied map[string]PenaltyAttribution
}

// NewPenaltyTracker creates a new penalty tracker.
func NewPenaltyTracker() *PenaltyTracker {
	return &PenaltyTracker{
		applied: make(map[string]PenaltyAttribution),
	}
}

// Apply attempts to apply a penalty, returning the amount if applied or 0 if already applied.
func (pt *PenaltyTracker) Apply(issueID string, category MetricCategory, amount float32, issue string) float32 {
	if _, exists := pt.applied[issueID]; exists {
		return 0
	}

	pt.applied[issueID] = PenaltyAttribution{
		SourceMetric: category,
		Amount:       amount,
		AppliedTo:    []MetricCategory{category},
		Issue:        issue,
	}

	return amount
}

// GetAttributions returns all applied penalty attributions.
func (pt *PenaltyTracker) GetAttributions() []PenaltyAttribution {
	result := make([]PenaltyAttribution, 0, len(pt.applied))
	for _, attr := range pt.applied {
		result = append(result, attr)
	}
	return result
}

// TdgScore represents a TDG score (0-100, higher is better).
type TdgScore struct {
	// Component scores (each contributes to the 100-point total)
	StructuralComplexity float32 `json:"structural_complexity"` // Max 25 points
	SemanticComplexity   float32 `json:"semantic_complexity"`   // Max 20 points
	DuplicationRatio     float32 `json:"duplication_ratio"`     // Max 20 points
	CouplingScore        float32 `json:"coupling_score"`        // Max 15 points
	DocCoverage          float32 `json:"doc_coverage"`          // Max 10 points
	ConsistencyScore     float32 `json:"consistency_score"`     // Max 10 points
	EntropyScore         float32 `json:"entropy_score"`         // Max 10 points (pattern entropy)

	// Aggregated score and grade
	Total float32 `json:"total"` // 0-100 (higher is better)
	Grade Grade   `json:"grade"` // A+ to F

	// Metadata
	Confidence           float32  `json:"confidence"`             // 0-1 confidence in the score
	Language             Language `json:"language"`               // Detected language
	FilePath             string   `json:"file_path,omitempty"`    // Source file path
	CriticalDefectsCount int      `json:"critical_defects_count"` // Count of critical defects
	HasCriticalDefects   bool     `json:"has_critical_defects"`   // Auto-fail flag

	// Penalty tracking for transparency
	PenaltiesApplied []PenaltyAttribution `json:"penalties_applied,omitempty"`
}

// NewTdgScore creates a new TDG score with default values.
func NewTdgScore() TdgScore {
	return TdgScore{
		StructuralComplexity: 25.0,
		SemanticComplexity:   20.0,
		DuplicationRatio:     20.0,
		CouplingScore:        15.0,
		DocCoverage:          10.0,
		ConsistencyScore:     10.0,
		EntropyScore:         0.0,
		Total:                100.0,
		Grade:                GradeAPlus,
		Confidence:           1.0,
		Language:             LanguageUnknown,
	}
}

// CalculateTotal computes the total score and grade from components.
func (s *TdgScore) CalculateTotal() {
	// Clamp individual components to their expected weight ranges
	s.StructuralComplexity = clampFloat32(s.StructuralComplexity, 0, 25)
	s.SemanticComplexity = clampFloat32(s.SemanticComplexity, 0, 20)
	s.DuplicationRatio = clampFloat32(s.DuplicationRatio, 0, 20)
	s.CouplingScore = clampFloat32(s.CouplingScore, 0, 15)
	s.DocCoverage = clampFloat32(s.DocCoverage, 0, 10)
	s.ConsistencyScore = clampFloat32(s.ConsistencyScore, 0, 10)
	s.EntropyScore = clampFloat32(s.EntropyScore, 0, 10)

	// Sum all clamped components
	rawTotal := s.StructuralComplexity +
		s.SemanticComplexity +
		s.DuplicationRatio +
		s.CouplingScore +
		s.DocCoverage +
		s.ConsistencyScore +
		s.EntropyScore

	// Normalize to 0-100 scale
	if rawTotal <= 100.0 {
		s.Total = clampFloat32(rawTotal, 0, 100)
	} else {
		// Scale down proportionally when entropy pushes total above 100
		const theoreticalMax float32 = 110.0 // 25+20+20+15+10+10+10
		s.Total = clampFloat32(rawTotal/theoreticalMax*100.0, 0, 100)
	}

	// Auto-fail if critical defects detected
	if s.HasCriticalDefects {
		s.Total = 0.0
		s.Grade = GradeF
	} else {
		s.Grade = GradeFromScore(s.Total)
	}
}

// SetMetric sets a metric value by category.
func (s *TdgScore) SetMetric(category MetricCategory, value float32) {
	switch category {
	case MetricStructuralComplexity:
		s.StructuralComplexity = value
	case MetricSemanticComplexity:
		s.SemanticComplexity = value
	case MetricDuplication:
		s.DuplicationRatio = value
	case MetricCoupling:
		s.CouplingScore = value
	case MetricDocumentation:
		s.DocCoverage = value
	case MetricConsistency:
		s.ConsistencyScore = value
	}
}

func clampFloat32(value, min, max float32) float32 {
	if value < min {
		return min
	}
	if value > max {
		return max
	}
	return value
}

// ProjectScore represents aggregated TDG scores for a project.
type ProjectScore struct {
	Files                []TdgScore       `json:"files"`
	AverageScore         float32          `json:"average_score"`
	AverageGrade         Grade            `json:"average_grade"`
	TotalFiles           int              `json:"total_files"`
	LanguageDistribution map[Language]int `json:"language_distribution"`
}

// AggregateProjectScore creates a ProjectScore from individual file scores.
func AggregateProjectScore(scores []TdgScore) ProjectScore {
	totalFiles := len(scores)
	var averageScore float32

	if totalFiles > 0 {
		var sum float32
		for _, s := range scores {
			sum += s.Total
		}
		averageScore = sum / float32(totalFiles)
	}

	langDist := make(map[Language]int)
	for _, s := range scores {
		langDist[s.Language]++
	}

	return ProjectScore{
		Files:                scores,
		AverageScore:         averageScore,
		AverageGrade:         GradeFromScore(averageScore),
		TotalFiles:           totalFiles,
		LanguageDistribution: langDist,
	}
}

// Average returns the average TDG score across all files.
func (p *ProjectScore) Average() TdgScore {
	if len(p.Files) == 0 {
		return NewTdgScore()
	}

	count := float32(len(p.Files))
	avg := NewTdgScore()

	var structSum, semSum, dupSum, coupSum, docSum, consSum, entSum, confSum float32
	for _, s := range p.Files {
		structSum += s.StructuralComplexity
		semSum += s.SemanticComplexity
		dupSum += s.DuplicationRatio
		coupSum += s.CouplingScore
		docSum += s.DocCoverage
		consSum += s.ConsistencyScore
		entSum += s.EntropyScore
		confSum += s.Confidence
	}

	avg.StructuralComplexity = structSum / count
	avg.SemanticComplexity = semSum / count
	avg.DuplicationRatio = dupSum / count
	avg.CouplingScore = coupSum / count
	avg.DocCoverage = docSum / count
	avg.ConsistencyScore = consSum / count
	avg.EntropyScore = entSum / count
	avg.Confidence = confSum / count

	avg.CalculateTotal()
	return avg
}

// TdgComparison represents a comparison between two TDG scores.
type TdgComparison struct {
	Source1               TdgScore `json:"source1"`
	Source2               TdgScore `json:"source2"`
	Delta                 float32  `json:"delta"`
	ImprovementPercentage float32  `json:"improvement_percentage"`
	Winner                string   `json:"winner"`
	Improvements          []string `json:"improvements"`
	Regressions           []string `json:"regressions"`
}

// NewTdgComparison creates a comparison between two scores.
func NewTdgComparison(source1, source2 TdgScore) TdgComparison {
	delta := source2.Total - source1.Total
	var improvementPct float32
	if source1.Total > 0 {
		improvementPct = (delta / source1.Total) * 100.0
	}

	winner := source2.FilePath
	if source1.Total >= source2.Total {
		winner = source1.FilePath
	}
	if winner == "" {
		if source2.Total > source1.Total {
			winner = "source2"
		} else {
			winner = "source1"
		}
	}

	var improvements, regressions []string

	// Compare structural complexity
	if source2.StructuralComplexity > source1.StructuralComplexity {
		improvements = append(improvements, "Structural complexity improved")
	} else if source2.StructuralComplexity < source1.StructuralComplexity {
		regressions = append(regressions, "Structural complexity degraded")
	}

	// Compare semantic complexity
	if source2.SemanticComplexity > source1.SemanticComplexity {
		improvements = append(improvements, "Semantic complexity improved")
	} else if source2.SemanticComplexity < source1.SemanticComplexity {
		regressions = append(regressions, "Semantic complexity degraded")
	}

	// Compare duplication
	if source2.DuplicationRatio > source1.DuplicationRatio {
		improvements = append(improvements, "Code duplication reduced")
	} else if source2.DuplicationRatio < source1.DuplicationRatio {
		regressions = append(regressions, "Code duplication increased")
	}

	// Compare documentation
	if source2.DocCoverage > source1.DocCoverage {
		improvements = append(improvements, "Documentation coverage improved")
	} else if source2.DocCoverage < source1.DocCoverage {
		regressions = append(regressions, "Documentation coverage decreased")
	}

	return TdgComparison{
		Source1:               source1,
		Source2:               source2,
		Delta:                 delta,
		ImprovementPercentage: improvementPct,
		Winner:                winner,
		Improvements:          improvements,
		Regressions:           regressions,
	}
}
