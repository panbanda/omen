package commit

import (
	"time"

	"github.com/panbanda/omen/pkg/analyzer/complexity"
)

// CommitAnalysis holds analysis results for a specific commit.
type CommitAnalysis struct {
	CommitHash string
	CommitDate time.Time
	Complexity *complexity.Analysis
}

// TrendPoint represents a single point in a metric trend.
type TrendPoint struct {
	CommitHash string
	Date       time.Time
	Value      float64
}

// Trend represents a metric's evolution over time.
type Trend struct {
	Metric string
	Points []TrendPoint
	Delta  float64 // Change from first to last point
}

// TrendAnalysis holds trend data for multiple metrics.
type TrendAnalysis struct {
	Commits             []*CommitAnalysis
	AvgCyclomaticTrend  Trend
	AvgCognitiveTrend   Trend
	TotalFilesTrend     Trend
	TotalFunctionsTrend Trend
}
