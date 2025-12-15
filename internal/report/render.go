package report

import (
	"embed"
	"encoding/json"
	"html/template"
	"io"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/text/cases"
	"golang.org/x/text/language"
	"golang.org/x/text/message"
)

//go:embed template.html
var templateFS embed.FS

// RenderData contains all data needed to render the report.
type RenderData struct {
	Metadata           Metadata
	Score              ScoreData
	ScoreClass         string
	Complexity         *ComplexityData
	Hotspots           *HotspotsData
	SATD               *SATDData
	Churn              *ChurnData
	Ownership          *OwnershipData
	Duplicates         *DuplicatesData
	Trend              *TrendData
	Summary            *SummaryInsight
	HotspotsInsight    *HotspotsInsight
	SATDInsight        *SATDInsight
	TrendsInsight      *TrendsInsight
	ChurnInsight       *ChurnInsight
	DuplicationInsight *DuplicationInsight
	ComponentsInsight  *ComponentsInsight
	ComponentTrends    map[string]ComponentTrendStats
	SATDStats          *SATDStats
}

// ScoreData represents the score.json structure.
type ScoreData struct {
	Score         int            `json:"score"`
	Passed        bool           `json:"passed"`
	FilesAnalyzed int            `json:"files_analyzed"`
	Components    map[string]int `json:"components"`
}

// ComplexityData represents complexity statistics.
type ComplexityData struct {
	AvgCyclomatic float64
	AvgCognitive  float64
}

// HotspotsData represents the hotspots.json structure.
type HotspotsData struct {
	Files   []HotspotItem   `json:"files"`
	Summary *HotspotSummary `json:"summary"`
}

// HotspotSummary contains aggregate hotspot stats.
type HotspotSummary struct {
	MaxScore float64
	AvgScore float64
}

// HotspotItem represents a single hotspot.
type HotspotItem struct {
	Path         string  `json:"path"`
	HotspotScore float64 `json:"hotspot_score"`
	Commits      int     `json:"commits"`
	AvgCognitive float64 `json:"avg_cognitive"`
}

// SATDData represents the satd.json structure.
type SATDData struct {
	Items []SATDItem `json:"items"`
}

// SATDItem represents a single SATD item.
type SATDItem struct {
	File     string `json:"file"`
	Line     int    `json:"line"`
	Severity string `json:"severity"`
	Category string `json:"category"`
	Content  string `json:"content"`
}

// SATDStats contains SATD statistics for charts.
type SATDStats struct {
	Critical   int            `json:"critical"`
	High       int            `json:"high"`
	Medium     int            `json:"medium"`
	Low        int            `json:"low"`
	Categories map[string]int `json:"categories"`
}

// ChurnData represents the churn.json structure.
type ChurnData struct {
	Files   []ChurnFile `json:"files"`
	Summary ChurnSummary
}

// ChurnSummary contains aggregate churn metrics (computed from files).
type ChurnSummary struct {
	TotalCommits int
	UniqueFiles  int
	TotalAdded   int
	TotalDeleted int
}

// ChurnFile represents a single file's churn data.
type ChurnFile struct {
	File       string   `json:"relative_path"`
	Commits    int      `json:"commit_count"`
	Authors    []string `json:"unique_authors"`
	ChurnScore float64  `json:"churn_score"`
	Additions  int      `json:"additions"`
	Deletions  int      `json:"deletions"`
}

// OwnershipData represents the ownership.json structure.
type OwnershipData struct {
	Summary        OwnershipSummary `json:"summary"`
	Files          []OwnershipFile  `json:"files"`
	BusFactor      int
	KnowledgeSilos int
	TotalFiles     int
	TopOwners      []OwnerInfo
}

// OwnershipSummary contains aggregate ownership metrics.
type OwnershipSummary struct {
	TotalFiles      int              `json:"total_files"`
	BusFactor       int              `json:"bus_factor"`
	SiloCount       int              `json:"silo_count"`
	TopContributors []TopContributor `json:"top_contributors"`
}

// TopContributor represents a top contributor's stats.
type TopContributor struct {
	Name  string `json:"name"`
	Files int    `json:"files"`
}

// OwnershipFile represents a single file's ownership.
type OwnershipFile struct {
	File string `json:"file"`
}

// OwnerInfo represents a code owner's stats for display.
type OwnerInfo struct {
	Name       string
	FilesOwned int
}

// DuplicatesData represents the duplicates.json structure.
type DuplicatesData struct {
	Summary          DuplicatesSummary `json:"summary"`
	CloneGroups      int
	DuplicateLines   int
	TotalLines       int
	DuplicationRatio float64
}

// DuplicatesSummary contains aggregate duplication metrics.
type DuplicatesSummary struct {
	TotalGroups      int     `json:"total_groups"`
	DuplicatedLines  int     `json:"duplicated_lines"`
	TotalLines       int     `json:"total_lines"`
	DuplicationRatio float64 `json:"duplication_ratio"`
}

// TrendData represents the trend.json structure.
type TrendData struct {
	Points          []TrendPoint                   `json:"points"`
	Slope           float64                        `json:"slope"`
	Intercept       float64                        `json:"intercept"`
	RSquared        float64                        `json:"r_squared"`
	StartScore      int                            `json:"start_score"`
	EndScore        int                            `json:"end_score"`
	ComponentTrends map[string]ComponentTrendStats `json:"component_trends"`
}

// TrendPoint represents a single point in time.
type TrendPoint struct {
	Date       string         `json:"date"`
	Score      int            `json:"score"`
	Components map[string]int `json:"components"`
}

// ComponentTrendStats contains trend statistics for a component.
type ComponentTrendStats struct {
	Slope       float64 `json:"Slope"`
	Correlation float64 `json:"Correlation"`
}

// Renderer handles HTML report generation.
type Renderer struct {
	tmpl *template.Template
}

// NewRenderer creates a new renderer with the embedded template.
func NewRenderer() (*Renderer, error) {
	funcMap := template.FuncMap{
		"scoreClass": func(score int) string {
			if score >= 80 {
				return "good"
			}
			if score >= 60 {
				return "warning"
			}
			return "danger"
		},
		"hotspotBadge": func(score float64) string {
			if score >= 0.7 {
				return "critical"
			}
			if score >= 0.5 {
				return "high"
			}
			if score >= 0.3 {
				return "medium"
			}
			return "low"
		},
		"churnBadge": func(score float64) string {
			// Churn scores are relative to repo max. P95 is typically ~0.01.
			// A score of 0.3+ means the file has significant portion of repo's total churn.
			if score >= 0.3 {
				return "critical"
			}
			if score >= 0.1 {
				return "high"
			}
			if score >= 0.02 {
				return "medium"
			}
			return "low"
		},
		"limit": func(items interface{}, n int) interface{} {
			switch v := items.(type) {
			case []HotspotItem:
				if len(v) > n {
					return v[:n]
				}
				return v
			case []SATDItem:
				if len(v) > n {
					return v[:n]
				}
				return v
			case []ChurnFile:
				if len(v) > n {
					return v[:n]
				}
				return v
			default:
				return items
			}
		},
		"lower": strings.ToLower,
		"title": cases.Title(language.English).String,
		"truncate": func(s string, n int) string {
			if len(s) > n {
				return s[:n] + "..."
			}
			return s
		},
		"truncatePath": func(s string, n int) string {
			if len(s) <= n {
				return s
			}
			parts := strings.Split(s, "/")
			if len(parts) <= 2 {
				return s[:n-3] + "..."
			}
			filename := parts[len(parts)-1]
			if len(filename) >= n-3 {
				return "..." + filename[len(filename)-n+3:]
			}
			remaining := n - len(filename) - 4
			if remaining < 0 {
				remaining = 0
			}
			prefix := strings.Join(parts[:len(parts)-1], "/")
			if len(prefix) > remaining {
				prefix = prefix[len(prefix)-remaining:]
			}
			return ".../" + prefix + "/" + filename
		},
		"percent": func(a, b int) float64 {
			if b == 0 {
				return 0
			}
			return float64(a) / float64(b) * 100
		},
		"countSeverity": func(items []SATDItem, severity string) int {
			count := 0
			for _, item := range items {
				if strings.EqualFold(item.Severity, severity) {
					count++
				}
			}
			return count
		},
		"filterSeverity": func(items []SATDItem, severities ...string) []SATDItem {
			var result []SATDItem
			severitySet := make(map[string]bool)
			for _, s := range severities {
				severitySet[strings.ToLower(s)] = true
			}
			for _, item := range items {
				if severitySet[strings.ToLower(item.Severity)] {
					result = append(result, item)
				}
			}
			return result
		},
		"json": func(v interface{}) template.JS {
			b, _ := json.Marshal(v)
			return template.JS(b)
		},
		"num": func(n interface{}) string {
			p := message.NewPrinter(language.English)
			switch v := n.(type) {
			case int:
				return p.Sprintf("%d", v)
			case int64:
				return p.Sprintf("%d", v)
			case float64:
				return p.Sprintf("%d", int64(v))
			default:
				return "0"
			}
		},
	}

	tmplContent, err := templateFS.ReadFile("template.html")
	if err != nil {
		return nil, err
	}

	tmpl, err := template.New("report").Funcs(funcMap).Parse(string(tmplContent))
	if err != nil {
		return nil, err
	}

	return &Renderer{tmpl: tmpl}, nil
}

// Render generates HTML from the data directory and writes to the output.
func (r *Renderer) Render(dataDir string, w io.Writer) error {
	data, err := r.loadData(dataDir)
	if err != nil {
		return err
	}

	return r.tmpl.Execute(w, data)
}

// RenderToFile generates HTML and writes it to a file.
func (r *Renderer) RenderToFile(dataDir, outputPath string) error {
	f, err := os.Create(outputPath)
	if err != nil {
		return err
	}
	defer f.Close()

	return r.Render(dataDir, f)
}

func (r *Renderer) loadData(dataDir string) (*RenderData, error) {
	data := &RenderData{}

	// Load metadata
	if err := loadJSON(filepath.Join(dataDir, "metadata.json"), &data.Metadata); err != nil {
		return nil, err
	}

	// Load score
	if err := loadJSON(filepath.Join(dataDir, "score.json"), &data.Score); err != nil {
		return nil, err
	}

	// Set score class
	if data.Score.Score >= 80 {
		data.ScoreClass = "good"
	} else if data.Score.Score >= 60 {
		data.ScoreClass = "warning"
	} else {
		data.ScoreClass = "danger"
	}

	// Load complexity (optional) and compute averages
	var complexityRaw struct {
		Summary struct {
			AvgCyclomatic float64 `json:"avg_cyclomatic"`
			AvgCognitive  float64 `json:"avg_cognitive"`
		} `json:"summary"`
	}
	if err := loadJSON(filepath.Join(dataDir, "complexity.json"), &complexityRaw); err == nil {
		data.Complexity = &ComplexityData{
			AvgCyclomatic: complexityRaw.Summary.AvgCyclomatic,
			AvgCognitive:  complexityRaw.Summary.AvgCognitive,
		}
	}

	// Load hotspots (optional) and compute summary
	hotspots := &HotspotsData{}
	if err := loadJSON(filepath.Join(dataDir, "hotspots.json"), hotspots); err == nil {
		// Compute summary if not present
		if hotspots.Summary == nil && len(hotspots.Files) > 0 {
			var maxScore, totalScore float64
			for _, f := range hotspots.Files {
				if f.HotspotScore > maxScore {
					maxScore = f.HotspotScore
				}
				totalScore += f.HotspotScore
			}
			hotspots.Summary = &HotspotSummary{
				MaxScore: maxScore,
				AvgScore: totalScore / float64(len(hotspots.Files)),
			}
		}
		data.Hotspots = hotspots
	}

	// Load SATD (optional) and compute stats
	satd := &SATDData{}
	if err := loadJSON(filepath.Join(dataDir, "satd.json"), satd); err == nil {
		data.SATD = satd
		// Compute stats for charts
		stats := &SATDStats{Categories: make(map[string]int)}
		for _, item := range satd.Items {
			switch strings.ToLower(item.Severity) {
			case "critical":
				stats.Critical++
			case "high":
				stats.High++
			case "medium":
				stats.Medium++
			default:
				stats.Low++
			}
			if item.Category != "" {
				stats.Categories[item.Category]++
			}
		}
		data.SATDStats = stats
	}

	// Load churn (optional) and compute summary
	churn := &ChurnData{}
	if err := loadJSON(filepath.Join(dataDir, "churn.json"), churn); err == nil {
		churn.Summary.UniqueFiles = len(churn.Files)
		for _, f := range churn.Files {
			churn.Summary.TotalCommits += f.Commits
			churn.Summary.TotalAdded += f.Additions
			churn.Summary.TotalDeleted += f.Deletions
		}
		data.Churn = churn
	}

	// Load ownership (optional) and transform for display
	ownership := &OwnershipData{}
	if err := loadJSON(filepath.Join(dataDir, "ownership.json"), ownership); err == nil {
		ownership.BusFactor = ownership.Summary.BusFactor
		ownership.KnowledgeSilos = ownership.Summary.SiloCount
		ownership.TotalFiles = ownership.Summary.TotalFiles
		for _, c := range ownership.Summary.TopContributors {
			ownership.TopOwners = append(ownership.TopOwners, OwnerInfo{
				Name:       c.Name,
				FilesOwned: c.Files,
			})
		}
		data.Ownership = ownership
	}

	// Load duplicates (optional) and transform for display
	duplicates := &DuplicatesData{}
	if err := loadJSON(filepath.Join(dataDir, "duplicates.json"), duplicates); err == nil {
		duplicates.CloneGroups = duplicates.Summary.TotalGroups
		duplicates.DuplicateLines = duplicates.Summary.DuplicatedLines
		duplicates.TotalLines = duplicates.Summary.TotalLines
		duplicates.DuplicationRatio = duplicates.Summary.DuplicationRatio * 100 // Convert to percentage
		data.Duplicates = duplicates
	}

	// Load trend (optional)
	trend := &TrendData{}
	if err := loadJSON(filepath.Join(dataDir, "trend.json"), trend); err == nil {
		data.Trend = trend
		data.ComponentTrends = trend.ComponentTrends
	}

	// Load insights if available
	insightsDir := filepath.Join(dataDir, "insights")
	if _, err := os.Stat(insightsDir); err == nil {
		// Load summary insight
		summary := &SummaryInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "summary.json"), summary); err == nil {
			data.Summary = summary
		}

		// Load hotspots insight
		hsInsight := &HotspotsInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "hotspots.json"), hsInsight); err == nil {
			data.HotspotsInsight = hsInsight
		}

		// Load SATD insight
		satdInsight := &SATDInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "satd.json"), satdInsight); err == nil {
			data.SATDInsight = satdInsight
		}

		// Load trends insight
		trendsInsight := &TrendsInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "trends.json"), trendsInsight); err == nil {
			data.TrendsInsight = trendsInsight
		}

		// Load churn insight
		churnInsight := &ChurnInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "churn.json"), churnInsight); err == nil {
			data.ChurnInsight = churnInsight
		}

		// Load duplication insight
		dupInsight := &DuplicationInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "duplication.json"), dupInsight); err == nil {
			data.DuplicationInsight = dupInsight
		}

		// Load components insight
		compInsight := &ComponentsInsight{}
		if err := loadJSON(filepath.Join(insightsDir, "components.json"), compInsight); err == nil {
			data.ComponentsInsight = compInsight
		}
	}

	return data, nil
}

func loadJSON(path string, v interface{}) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()

	return json.NewDecoder(f).Decode(v)
}
