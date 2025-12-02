package cohesion

import (
	"sort"
	"time"
)

// ClassMetrics represents CK metrics for a single class.
type ClassMetrics struct {
	Path      string `json:"path"`
	ClassName string `json:"class_name"`
	Language  string `json:"language"`
	StartLine int    `json:"start_line"`
	EndLine   int    `json:"end_line"`

	// Weighted Methods per Class - sum of cyclomatic complexity of all methods
	WMC int `json:"wmc"`

	// Coupling Between Objects - number of other classes referenced
	CBO int `json:"cbo"`

	// Response For Class - number of methods that can be executed in response to a message
	RFC int `json:"rfc"`

	// Lack of Cohesion in Methods (LCOM4) - number of connected components in method-field graph
	// Lower is better; 1 = fully cohesive, >1 = could be split
	LCOM int `json:"lcom"`

	// Depth of Inheritance Tree
	DIT int `json:"dit"`

	// Number of Children (direct subclasses)
	NOC int `json:"noc"`

	// Number of methods
	NOM int `json:"nom"`

	// Number of fields/attributes
	NOF int `json:"nof"`

	// Lines of code in the class
	LOC int `json:"loc"`

	// Method names for reference
	Methods []string `json:"methods,omitempty"`

	// Field names for reference
	Fields []string `json:"fields,omitempty"`

	// Classes this class couples to
	CoupledClasses []string `json:"coupled_classes,omitempty"`
}

// Summary provides aggregate CK metrics.
type Summary struct {
	TotalClasses int     `json:"total_classes"`
	TotalFiles   int     `json:"total_files"`
	AvgWMC       float64 `json:"avg_wmc"`
	AvgCBO       float64 `json:"avg_cbo"`
	AvgRFC       float64 `json:"avg_rfc"`
	AvgLCOM      float64 `json:"avg_lcom"`
	MaxWMC       int     `json:"max_wmc"`
	MaxCBO       int     `json:"max_cbo"`
	MaxRFC       int     `json:"max_rfc"`
	MaxLCOM      int     `json:"max_lcom"`
	MaxDIT       int     `json:"max_dit"`

	// Classes with high LCOM (>1) that may need refactoring
	LowCohesionCount int `json:"low_cohesion_count"`
}

// Analysis represents the full CK metrics analysis result.
type Analysis struct {
	GeneratedAt time.Time      `json:"generated_at"`
	Classes     []ClassMetrics `json:"classes"`
	Summary     Summary        `json:"summary"`
}

// CalculateSummary computes summary statistics.
func (c *Analysis) CalculateSummary() {
	if len(c.Classes) == 0 {
		return
	}

	files := make(map[string]bool)
	var totalWMC, totalCBO, totalRFC, totalLCOM int

	for _, cls := range c.Classes {
		files[cls.Path] = true
		totalWMC += cls.WMC
		totalCBO += cls.CBO
		totalRFC += cls.RFC
		totalLCOM += cls.LCOM

		if cls.WMC > c.Summary.MaxWMC {
			c.Summary.MaxWMC = cls.WMC
		}
		if cls.CBO > c.Summary.MaxCBO {
			c.Summary.MaxCBO = cls.CBO
		}
		if cls.RFC > c.Summary.MaxRFC {
			c.Summary.MaxRFC = cls.RFC
		}
		if cls.LCOM > c.Summary.MaxLCOM {
			c.Summary.MaxLCOM = cls.LCOM
		}
		if cls.DIT > c.Summary.MaxDIT {
			c.Summary.MaxDIT = cls.DIT
		}
		if cls.LCOM > 1 {
			c.Summary.LowCohesionCount++
		}
	}

	n := float64(len(c.Classes))
	c.Summary.TotalClasses = len(c.Classes)
	c.Summary.TotalFiles = len(files)
	c.Summary.AvgWMC = float64(totalWMC) / n
	c.Summary.AvgCBO = float64(totalCBO) / n
	c.Summary.AvgRFC = float64(totalRFC) / n
	c.Summary.AvgLCOM = float64(totalLCOM) / n
}

// SortByLCOM sorts classes by LCOM in descending order (least cohesive first).
func (c *Analysis) SortByLCOM() {
	sort.Slice(c.Classes, func(i, j int) bool {
		return c.Classes[i].LCOM > c.Classes[j].LCOM
	})
}

// SortByWMC sorts classes by WMC in descending order (most complex first).
func (c *Analysis) SortByWMC() {
	sort.Slice(c.Classes, func(i, j int) bool {
		return c.Classes[i].WMC > c.Classes[j].WMC
	})
}

// SortByCBO sorts classes by CBO in descending order (most coupled first).
func (c *Analysis) SortByCBO() {
	sort.Slice(c.Classes, func(i, j int) bool {
		return c.Classes[i].CBO > c.Classes[j].CBO
	})
}

// SortByDIT sorts classes by DIT in descending order (deepest inheritance first).
func (c *Analysis) SortByDIT() {
	sort.Slice(c.Classes, func(i, j int) bool {
		return c.Classes[i].DIT > c.Classes[j].DIT
	})
}
