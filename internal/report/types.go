package report

import "time"

// Metadata contains report generation metadata.
type Metadata struct {
	Repository  string    `json:"repository"`
	GeneratedAt time.Time `json:"generated_at"`
	Since       string    `json:"since"`
	OmenVersion string    `json:"omen_version"`
	Paths       []string  `json:"paths"`
}

// Recommendation represents a single recommendation item.
type Recommendation struct {
	Title       string `json:"title"`
	Description string `json:"description"`
}

// Recommendations groups recommendations by priority.
type Recommendations struct {
	HighPriority   []Recommendation `json:"high_priority"`
	MediumPriority []Recommendation `json:"medium_priority"`
	Ongoing        []Recommendation `json:"ongoing"`
}

// SummaryInsight contains executive summary and recommendations.
type SummaryInsight struct {
	ExecutiveSummary string          `json:"executive_summary"`
	KeyFindings      []string        `json:"key_findings"`
	Recommendations  Recommendations `json:"recommendations"`
}

// ScoreAnnotation represents an annotation on the score trend chart.
type ScoreAnnotation struct {
	Date        string `json:"date"`
	Label       string `json:"label"`
	Change      int    `json:"change"`
	Description string `json:"description"`
}

// HistoricalEvent represents a significant score change event.
type HistoricalEvent struct {
	Period        string   `json:"period"`
	Change        int      `json:"change"`
	PrimaryDriver string   `json:"primary_driver"`
	Releases      []string `json:"releases"`
}

// TrendsInsight contains trend analysis and annotations.
type TrendsInsight struct {
	SectionInsight   string            `json:"section_insight"`
	ScoreAnnotations []ScoreAnnotation `json:"score_annotations"`
	HistoricalEvents []HistoricalEvent `json:"historical_events"`
}

// ComponentAnnotation represents an annotation on a component trend.
type ComponentAnnotation struct {
	Date        string `json:"date"`
	Label       string `json:"label"`
	From        int    `json:"from"`
	To          int    `json:"to"`
	Description string `json:"description"`
}

// ComponentEvent represents a significant component change.
type ComponentEvent struct {
	Period    string `json:"period"`
	Component string `json:"component"`
	From      int    `json:"from"`
	To        int    `json:"to"`
	Context   string `json:"context"`
}

// ComponentsInsight contains component-level analysis.
type ComponentsInsight struct {
	ComponentAnnotations map[string][]ComponentAnnotation `json:"component_annotations"`
	ComponentEvents      []ComponentEvent                 `json:"component_events"`
	ComponentInsights    map[string]string                `json:"component_insights"`
}

// Pattern represents a cross-cutting observation.
type Pattern struct {
	Category string `json:"category"`
	Insight  string `json:"insight"`
}

// PatternsInsight contains pattern analysis.
type PatternsInsight struct {
	Patterns []Pattern `json:"patterns"`
}

// FileAnnotation represents an LLM comment on a specific file.
type FileAnnotation struct {
	File    string `json:"file"`
	Comment string `json:"comment"`
}

// HotspotsInsight contains hotspot analysis.
type HotspotsInsight struct {
	SectionInsight  string           `json:"section_insight"`
	ItemAnnotations []FileAnnotation `json:"item_annotations"`
}

// OwnershipInsight contains ownership analysis.
type OwnershipInsight struct {
	SectionInsight  string           `json:"section_insight"`
	ItemAnnotations []FileAnnotation `json:"item_annotations"`
}

// SATDAnnotation represents an LLM comment on a SATD item.
type SATDAnnotation struct {
	File    string `json:"file"`
	Line    int    `json:"line,omitempty"`
	Comment string `json:"comment"`
}

// SATDInsight contains SATD analysis.
type SATDInsight struct {
	SectionInsight  string           `json:"section_insight"`
	ItemAnnotations []SATDAnnotation `json:"item_annotations"`
}

// ChurnInsight contains churn analysis.
type ChurnInsight struct {
	SectionInsight string `json:"section_insight"`
}

// DuplicationInsight contains duplication analysis.
type DuplicationInsight struct {
	SectionInsight string `json:"section_insight"`
}

// FlagAnnotation represents an LLM comment on a feature flag.
type FlagAnnotation struct {
	Flag         string `json:"flag"`
	Priority     string `json:"priority"`      // CRITICAL, HIGH, MEDIUM, LOW
	IntroducedAt string `json:"introduced_at"` // ISO 8601 timestamp
	Comment      string `json:"comment"`
}

// FlagsInsight contains feature flags analysis.
type FlagsInsight struct {
	SectionInsight  string           `json:"section_insight"`
	ItemAnnotations []FlagAnnotation `json:"item_annotations"`
}

// FlagsData represents the flags.json structure.
type FlagsData struct {
	Flags   []FlagItem   `json:"flags"`
	Summary FlagsSummary `json:"summary"`
}

// FlagItem represents a single feature flag.
type FlagItem struct {
	FlagKey    string         `json:"flag_key"`
	Provider   string         `json:"provider"`
	Priority   FlagPriority   `json:"priority"`
	Complexity FlagComplexity `json:"complexity"`
	Staleness  FlagStaleness  `json:"staleness"`
}

// FlagPriority contains flag priority scoring.
type FlagPriority struct {
	Score float64 `json:"score"`
	Level string  `json:"level"`
}

// FlagComplexity contains flag complexity metrics.
type FlagComplexity struct {
	FileSpread int `json:"file_spread"`
}

// FlagStaleness contains flag staleness metrics.
type FlagStaleness struct {
	IntroducedAt string `json:"introduced_at"`
}

// FlagsSummary contains aggregate flag metrics.
type FlagsSummary struct {
	TotalFlags    int            `json:"total_flags"`
	ByPriority    map[string]int `json:"by_priority"`
	ByProvider    map[string]int `json:"by_provider"`
	AvgFileSpread float64        `json:"avg_file_spread"`
}

// CohesionData represents the cohesion.json structure.
type CohesionData struct {
	Classes []CohesionClass `json:"classes"`
}

// CohesionClass represents a single class's CK metrics.
type CohesionClass struct {
	Path      string `json:"path"`
	ClassName string `json:"class_name"`
	Language  string `json:"language"`
	WMC       int    `json:"wmc"`
	CBO       int    `json:"cbo"`
	RFC       int    `json:"rfc"`
	LCOM      int    `json:"lcom"`
	DIT       int    `json:"dit"`
	NOC       int    `json:"noc"`
	NOM       int    `json:"nom"`
	NOF       int    `json:"nof"`
	LOC       int    `json:"loc"`
}
