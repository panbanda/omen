package tdg

import (
	"math"
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

// Score represents a TDG score (0-100, higher is better).
type Score struct {
	// Component scores (each contributes to the 100-point total)
	StructuralComplexity  float32 `json:"structural_complexity"`   // Max 20 points
	SemanticComplexity    float32 `json:"semantic_complexity"`     // Max 15 points
	DuplicationRatio      float32 `json:"duplication_ratio"`       // Max 15 points
	CouplingScore         float32 `json:"coupling_score"`          // Max 15 points
	DocCoverage           float32 `json:"doc_coverage"`            // Max 5 points
	ConsistencyScore      float32 `json:"consistency_score"`       // Max 10 points
	HotspotScore          float32 `json:"hotspot_score"`           // Max 10 points (churn x complexity)
	TemporalCouplingScore float32 `json:"temporal_coupling_score"` // Max 10 points (co-change patterns)
	EntropyScore          float32 `json:"entropy_score"`           // Max 10 points (pattern entropy)

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

// NewScore creates a new TDG score with default values.
func NewScore() Score {
	return Score{
		StructuralComplexity:  20.0,
		SemanticComplexity:    15.0,
		DuplicationRatio:      15.0,
		CouplingScore:         15.0,
		DocCoverage:           5.0,
		ConsistencyScore:      10.0,
		HotspotScore:          10.0, // Default: no hotspot penalty
		TemporalCouplingScore: 10.0, // Default: no temporal coupling penalty
		EntropyScore:          0.0,
		Total:                 100.0,
		Grade:                 GradeAPlus,
		Confidence:            1.0,
		Language:              LanguageUnknown,
	}
}

// CalculateTotal computes the total score and grade from components.
func (s *Score) CalculateTotal() {
	// Clamp individual components to their expected weight ranges
	s.StructuralComplexity = clampFloat32(s.StructuralComplexity, 0, 20)
	s.SemanticComplexity = clampFloat32(s.SemanticComplexity, 0, 15)
	s.DuplicationRatio = clampFloat32(s.DuplicationRatio, 0, 15)
	s.CouplingScore = clampFloat32(s.CouplingScore, 0, 15)
	s.DocCoverage = clampFloat32(s.DocCoverage, 0, 5)
	s.ConsistencyScore = clampFloat32(s.ConsistencyScore, 0, 10)
	s.HotspotScore = clampFloat32(s.HotspotScore, 0, 10)
	s.TemporalCouplingScore = clampFloat32(s.TemporalCouplingScore, 0, 10)
	s.EntropyScore = clampFloat32(s.EntropyScore, 0, 10)

	// Sum all clamped components
	rawTotal := s.StructuralComplexity +
		s.SemanticComplexity +
		s.DuplicationRatio +
		s.CouplingScore +
		s.DocCoverage +
		s.ConsistencyScore +
		s.HotspotScore +
		s.TemporalCouplingScore +
		s.EntropyScore

	// Normalize to 0-100 scale
	// Base max is 100 (20+15+15+15+5+10+10+10), but entropy can add 10 more
	if rawTotal <= 100.0 {
		s.Total = clampFloat32(rawTotal, 0, 100)
	} else {
		// Scale down proportionally when entropy pushes total above 100
		const theoreticalMax float32 = 110.0 // 20+15+15+15+5+10+10+10+10
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
func (s *Score) SetMetric(category MetricCategory, value float32) {
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
	Files                []Score          `json:"files"`
	AverageScore         float32          `json:"average_score"`
	AverageGrade         Grade            `json:"average_grade"`
	TotalFiles           int              `json:"total_files"`
	LanguageDistribution map[Language]int `json:"language_distribution"`
	GradeDistribution    map[Grade]int    `json:"grade_distribution"`
}

// AggregateProjectScore creates a ProjectScore from individual file scores.
func AggregateProjectScore(scores []Score) ProjectScore {
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
	gradeDist := make(map[Grade]int)
	for _, s := range scores {
		langDist[s.Language]++
		gradeDist[s.Grade]++
	}

	return ProjectScore{
		Files:                scores,
		AverageScore:         averageScore,
		AverageGrade:         GradeFromScore(averageScore),
		TotalFiles:           totalFiles,
		LanguageDistribution: langDist,
		GradeDistribution:    gradeDist,
	}
}

// Average returns the average TDG score across all files.
func (p *ProjectScore) Average() Score {
	if len(p.Files) == 0 {
		return NewScore()
	}

	count := float32(len(p.Files))
	avg := NewScore()

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

// Comparison represents a comparison between two TDG scores.
type Comparison struct {
	Source1               Score    `json:"source1"`
	Source2               Score    `json:"source2"`
	Delta                 float32  `json:"delta"`
	ImprovementPercentage float32  `json:"improvement_percentage"`
	Winner                string   `json:"winner"`
	Improvements          []string `json:"improvements"`
	Regressions           []string `json:"regressions"`
}

// NewComparison creates a comparison between two scores.
func NewComparison(source1, source2 Score) Comparison {
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

	return Comparison{
		Source1:               source1,
		Source2:               source2,
		Delta:                 delta,
		ImprovementPercentage: improvementPct,
		Winner:                winner,
		Improvements:          improvements,
		Regressions:           regressions,
	}
}

// Severity represents the severity classification based on thresholds (pmat-compatible).
type Severity string

const (
	SeverityNormal   Severity = "normal"   // TDG < 1.5
	SeverityWarning  Severity = "warning"  // TDG 1.5-2.5
	SeverityCritical Severity = "critical" // TDG > 2.5
)

// SeverityFromValue converts a TDG value (0-5 scale) to severity.
func SeverityFromValue(value float64) Severity {
	if value > 2.5 {
		return SeverityCritical
	} else if value > 1.5 {
		return SeverityWarning
	}
	return SeverityNormal
}

// Hotspot represents a file with high technical debt (pmat-compatible). omen:ignore
type Hotspot struct {
	Path           string  `json:"path"`
	TdgScore       float64 `json:"tdg_score"`
	PrimaryFactor  string  `json:"primary_factor"`
	EstimatedHours float64 `json:"estimated_hours"`
}

// Summary provides aggregate statistics (pmat-compatible).
type Summary struct {
	TotalFiles         int     `json:"total_files"`
	CriticalFiles      int     `json:"critical_files"`
	WarningFiles       int     `json:"warning_files"`
	AverageTdg         float64 `json:"average_tdg"`
	P95Tdg             float64 `json:"p95_tdg"`
	P99Tdg             float64 `json:"p99_tdg"`
	EstimatedDebtHours float64 `json:"estimated_debt_hours"`
}

// Report is the pmat-compatible TDG analysis output.
type Report struct {
	Summary  Summary   `json:"summary"`
	Hotspots []Hotspot `json:"hotspots"`
}

// ToReport converts a ProjectScore to pmat-compatible Report.
// Omen uses 0-100 scale (higher = better quality), pmat uses 0-5 scale (higher = more debt).
func (p *ProjectScore) ToReport(topN int) *Report {
	report := &Report{
		Summary: Summary{
			TotalFiles: p.TotalFiles,
		},
		Hotspots: make([]Hotspot, 0),
	}

	if p.TotalFiles == 0 {
		return report
	}

	// Convert omen scores to pmat-style TDG values
	// Omen: 0-100 (100 = best), pmat: 0-5 (0 = best)
	// Formula: tdg = (100 - omenScore) / 20
	tdgValues := make([]float64, 0, len(p.Files))

	for _, f := range p.Files {
		tdgValue := (100.0 - float64(f.Total)) / 20.0
		if tdgValue < 0 {
			tdgValue = 0
		}
		if tdgValue > 5 {
			tdgValue = 5
		}
		tdgValues = append(tdgValues, tdgValue)

		severity := SeverityFromValue(tdgValue)
		if severity == SeverityCritical {
			report.Summary.CriticalFiles++
		} else if severity == SeverityWarning {
			report.Summary.WarningFiles++
		}

		// Identify primary factor
		primaryFactor := identifyPrimaryFactor(f)

		// Calculate estimated hours (pmat formula: 2.0 * 1.8^tdg)
		estimatedHours := 2.0 * math.Pow(1.8, tdgValue)

		report.Hotspots = append(report.Hotspots, Hotspot{
			Path:           f.FilePath,
			TdgScore:       tdgValue,
			PrimaryFactor:  primaryFactor,
			EstimatedHours: estimatedHours,
		})

		report.Summary.EstimatedDebtHours += estimatedHours
	}

	// Calculate average TDG
	var sum float64
	for _, v := range tdgValues {
		sum += v
	}
	report.Summary.AverageTdg = sum / float64(len(tdgValues))

	// Sort hotspots by TDG score (highest first = worst quality)
	sortHotspotsByScore(report.Hotspots)

	// Calculate percentiles
	sortedValues := make([]float64, len(tdgValues))
	copy(sortedValues, tdgValues)
	sortFloat64s(sortedValues)
	report.Summary.P95Tdg = percentile(sortedValues, 0.95)
	report.Summary.P99Tdg = percentile(sortedValues, 0.99)

	// Limit hotspots to topN
	if topN > 0 && len(report.Hotspots) > topN {
		report.Hotspots = report.Hotspots[:topN]
	}

	return report
}

// identifyPrimaryFactor determines the main contributor to high TDG.
func identifyPrimaryFactor(score Score) string {
	// Calculate weighted contributions (using pmat weights)
	// Complexity: 30%, Churn: 35%, Coupling: 15%, Domain Risk: 10%, Duplication: 10%
	// For omen, we use the component penalties as proxies
	structPenalty := 25.0 - float64(score.StructuralComplexity)
	semPenalty := 20.0 - float64(score.SemanticComplexity)
	couplingPenalty := 15.0 - float64(score.CouplingScore)
	dupPenalty := 20.0 - float64(score.DuplicationRatio)

	// Weighted contributions
	factors := []struct {
		name   string
		weight float64
	}{
		{"High Complexity", (structPenalty + semPenalty) * 0.30},
		{"High Coupling", couplingPenalty * 0.15},
		{"Code Duplication", dupPenalty * 0.10},
	}

	// Find highest contributor
	maxFactor := factors[0]
	for _, f := range factors[1:] {
		if f.weight > maxFactor.weight {
			maxFactor = f
		}
	}

	return maxFactor.name
}

// Helper functions for percentile calculation
func percentile(sorted []float64, p float64) float64 {
	if len(sorted) == 0 {
		return 0.0
	}
	idx := int(float64(len(sorted)) * p)
	if idx >= len(sorted) {
		idx = len(sorted) - 1
	}
	return sorted[idx]
}

func sortFloat64s(vals []float64) {
	for i := 0; i < len(vals)-1; i++ {
		for j := i + 1; j < len(vals); j++ {
			if vals[i] > vals[j] {
				vals[i], vals[j] = vals[j], vals[i]
			}
		}
	}
}

func sortHotspotsByScore(hotspots []Hotspot) {
	for i := 0; i < len(hotspots)-1; i++ {
		for j := i + 1; j < len(hotspots); j++ {
			if hotspots[i].TdgScore < hotspots[j].TdgScore {
				hotspots[i], hotspots[j] = hotspots[j], hotspots[i]
			}
		}
	}
}
