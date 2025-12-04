package commit

import (
	"fmt"
	"io"
	"strings"
	"time"
)

// RenderText implements output.Renderable for text output.
func (t *TrendAnalysis) RenderText(w io.Writer, colored bool) error {
	if len(t.Commits) == 0 {
		fmt.Fprintln(w, "No commits found in the specified time range")
		return nil
	}

	fmt.Fprintf(w, "Trend Analysis (%d commits)\n", len(t.Commits))
	fmt.Fprintln(w, strings.Repeat("=", 50))
	fmt.Fprintln(w)

	first := t.AvgCyclomaticTrend.Points[0]
	last := t.AvgCyclomaticTrend.Points[len(t.AvgCyclomaticTrend.Points)-1]

	fmt.Fprintf(w, "Time Range: %s to %s\n",
		first.Date.Format("2006-01-02"),
		last.Date.Format("2006-01-02"))
	fmt.Fprintln(w)

	fmt.Fprintf(w, "Avg Cyclomatic Complexity:  %.2f -> %.2f (delta: %+.2f)\n",
		first.Value,
		last.Value,
		t.AvgCyclomaticTrend.Delta)

	firstCog := t.AvgCognitiveTrend.Points[0]
	lastCog := t.AvgCognitiveTrend.Points[len(t.AvgCognitiveTrend.Points)-1]
	fmt.Fprintf(w, "Avg Cognitive Complexity:   %.2f -> %.2f (delta: %+.2f)\n",
		firstCog.Value,
		lastCog.Value,
		t.AvgCognitiveTrend.Delta)

	firstFiles := t.TotalFilesTrend.Points[0]
	lastFiles := t.TotalFilesTrend.Points[len(t.TotalFilesTrend.Points)-1]
	fmt.Fprintf(w, "Total Files:                %.0f -> %.0f (delta: %+.0f)\n",
		firstFiles.Value,
		lastFiles.Value,
		t.TotalFilesTrend.Delta)

	firstFuncs := t.TotalFunctionsTrend.Points[0]
	lastFuncs := t.TotalFunctionsTrend.Points[len(t.TotalFunctionsTrend.Points)-1]
	fmt.Fprintf(w, "Total Functions:            %.0f -> %.0f (delta: %+.0f)\n",
		firstFuncs.Value,
		lastFuncs.Value,
		t.TotalFunctionsTrend.Delta)

	fmt.Fprintln(w)
	return nil
}

// RenderMarkdown implements output.Renderable for markdown output.
func (t *TrendAnalysis) RenderMarkdown(w io.Writer) error {
	if len(t.Commits) == 0 {
		fmt.Fprintln(w, "No commits found in the specified time range")
		return nil
	}

	fmt.Fprintf(w, "## Trend Analysis (%d commits)\n\n", len(t.Commits))

	first := t.AvgCyclomaticTrend.Points[0]
	last := t.AvgCyclomaticTrend.Points[len(t.AvgCyclomaticTrend.Points)-1]

	fmt.Fprintf(w, "**Time Range:** %s to %s\n\n",
		first.Date.Format("2006-01-02"),
		last.Date.Format("2006-01-02"))

	fmt.Fprintln(w, "| Metric | Start | End | Delta |")
	fmt.Fprintln(w, "|--------|-------|-----|-------|")

	fmt.Fprintf(w, "| Avg Cyclomatic | %.2f | %.2f | %+.2f |\n",
		first.Value,
		last.Value,
		t.AvgCyclomaticTrend.Delta)

	firstCog := t.AvgCognitiveTrend.Points[0]
	lastCog := t.AvgCognitiveTrend.Points[len(t.AvgCognitiveTrend.Points)-1]
	fmt.Fprintf(w, "| Avg Cognitive | %.2f | %.2f | %+.2f |\n",
		firstCog.Value,
		lastCog.Value,
		t.AvgCognitiveTrend.Delta)

	firstFiles := t.TotalFilesTrend.Points[0]
	lastFiles := t.TotalFilesTrend.Points[len(t.TotalFilesTrend.Points)-1]
	fmt.Fprintf(w, "| Total Files | %.0f | %.0f | %+.0f |\n",
		firstFiles.Value,
		lastFiles.Value,
		t.TotalFilesTrend.Delta)

	firstFuncs := t.TotalFunctionsTrend.Points[0]
	lastFuncs := t.TotalFunctionsTrend.Points[len(t.TotalFunctionsTrend.Points)-1]
	fmt.Fprintf(w, "| Total Functions | %.0f | %.0f | %+.0f |\n",
		firstFuncs.Value,
		lastFuncs.Value,
		t.TotalFunctionsTrend.Delta)

	fmt.Fprintln(w)
	return nil
}

// RenderData implements output.Renderable for JSON/TOON output.
func (t *TrendAnalysis) RenderData() any {
	type trendData struct {
		TimeRange struct {
			Start time.Time `json:"start" toon:"start"`
			End   time.Time `json:"end" toon:"end"`
		} `json:"time_range" toon:"time_range"`
		Commits             int     `json:"commits" toon:"commits"`
		AvgCyclomaticStart  float64 `json:"avg_cyclomatic_start" toon:"avg_cyclomatic_start"`
		AvgCyclomaticEnd    float64 `json:"avg_cyclomatic_end" toon:"avg_cyclomatic_end"`
		AvgCyclomaticDelta  float64 `json:"avg_cyclomatic_delta" toon:"avg_cyclomatic_delta"`
		AvgCognitiveStart   float64 `json:"avg_cognitive_start" toon:"avg_cognitive_start"`
		AvgCognitiveEnd     float64 `json:"avg_cognitive_end" toon:"avg_cognitive_end"`
		AvgCognitiveDelta   float64 `json:"avg_cognitive_delta" toon:"avg_cognitive_delta"`
		TotalFilesStart     float64 `json:"total_files_start" toon:"total_files_start"`
		TotalFilesEnd       float64 `json:"total_files_end" toon:"total_files_end"`
		TotalFilesDelta     float64 `json:"total_files_delta" toon:"total_files_delta"`
		TotalFunctionsStart float64 `json:"total_functions_start" toon:"total_functions_start"`
		TotalFunctionsEnd   float64 `json:"total_functions_end" toon:"total_functions_end"`
		TotalFunctionsDelta float64 `json:"total_functions_delta" toon:"total_functions_delta"`
		DetailedTrends      struct {
			AvgCyclomatic  Trend `json:"avg_cyclomatic" toon:"avg_cyclomatic"`
			AvgCognitive   Trend `json:"avg_cognitive" toon:"avg_cognitive"`
			TotalFiles     Trend `json:"total_files" toon:"total_files"`
			TotalFunctions Trend `json:"total_functions" toon:"total_functions"`
		} `json:"detailed_trends" toon:"detailed_trends"`
	}

	if len(t.Commits) == 0 {
		return trendData{}
	}

	first := t.AvgCyclomaticTrend.Points[0]
	last := t.AvgCyclomaticTrend.Points[len(t.AvgCyclomaticTrend.Points)-1]

	data := trendData{
		Commits:             len(t.Commits),
		AvgCyclomaticStart:  first.Value,
		AvgCyclomaticEnd:    last.Value,
		AvgCyclomaticDelta:  t.AvgCyclomaticTrend.Delta,
		AvgCognitiveStart:   t.AvgCognitiveTrend.Points[0].Value,
		AvgCognitiveEnd:     t.AvgCognitiveTrend.Points[len(t.AvgCognitiveTrend.Points)-1].Value,
		AvgCognitiveDelta:   t.AvgCognitiveTrend.Delta,
		TotalFilesStart:     t.TotalFilesTrend.Points[0].Value,
		TotalFilesEnd:       t.TotalFilesTrend.Points[len(t.TotalFilesTrend.Points)-1].Value,
		TotalFilesDelta:     t.TotalFilesTrend.Delta,
		TotalFunctionsStart: t.TotalFunctionsTrend.Points[0].Value,
		TotalFunctionsEnd:   t.TotalFunctionsTrend.Points[len(t.TotalFunctionsTrend.Points)-1].Value,
		TotalFunctionsDelta: t.TotalFunctionsTrend.Delta,
	}

	data.TimeRange.Start = first.Date
	data.TimeRange.End = last.Date

	data.DetailedTrends.AvgCyclomatic = t.AvgCyclomaticTrend
	data.DetailedTrends.AvgCognitive = t.AvgCognitiveTrend
	data.DetailedTrends.TotalFiles = t.TotalFilesTrend
	data.DetailedTrends.TotalFunctions = t.TotalFunctionsTrend

	return data
}
