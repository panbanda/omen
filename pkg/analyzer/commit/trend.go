package commit

import "sort"

// CalculateTrends computes metric trends from commit analyses.
func CalculateTrends(commits []*CommitAnalysis) *TrendAnalysis {
	if len(commits) == 0 {
		return &TrendAnalysis{}
	}

	// Sort by date (oldest first)
	sorted := make([]*CommitAnalysis, len(commits))
	copy(sorted, commits)
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].CommitDate.Before(sorted[j].CommitDate)
	})

	analysis := &TrendAnalysis{
		Commits: sorted,
	}

	// Build trend points for each metric
	analysis.AvgCyclomaticTrend = buildTrend("avg_cyclomatic", sorted, func(c *CommitAnalysis) float64 {
		return c.Complexity.Summary.AvgCyclomatic
	})

	analysis.AvgCognitiveTrend = buildTrend("avg_cognitive", sorted, func(c *CommitAnalysis) float64 {
		return c.Complexity.Summary.AvgCognitive
	})

	analysis.TotalFilesTrend = buildTrend("total_files", sorted, func(c *CommitAnalysis) float64 {
		return float64(c.Complexity.Summary.TotalFiles)
	})

	analysis.TotalFunctionsTrend = buildTrend("total_functions", sorted, func(c *CommitAnalysis) float64 {
		return float64(c.Complexity.Summary.TotalFunctions)
	})

	return analysis
}

// buildTrend creates a Trend from commit analyses using an extractor function.
func buildTrend(metric string, commits []*CommitAnalysis, extract func(*CommitAnalysis) float64) Trend {
	points := make([]TrendPoint, len(commits))
	for i, c := range commits {
		points[i] = TrendPoint{
			CommitHash: c.CommitHash,
			Date:       c.CommitDate,
			Value:      extract(c),
		}
	}

	var delta float64
	if len(points) >= 2 {
		delta = points[len(points)-1].Value - points[0].Value
	}

	return Trend{
		Metric: metric,
		Points: points,
		Delta:  delta,
	}
}
