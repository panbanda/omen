package mcpserver

import (
	"context"
	"errors"
	"sort"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/panbanda/omen/internal/locator"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/changes"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/deadcode"
	"github.com/panbanda/omen/pkg/analyzer/defect"
	"github.com/panbanda/omen/pkg/analyzer/featureflags"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/repomap"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/panbanda/omen/pkg/analyzer/tdg"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/source"
	toon "github.com/toon-format/toon-go"
)

// Common input structures for tools

// AnalyzeInput is the base input for all analyze tools.
type AnalyzeInput struct {
	Paths  []string `json:"paths,omitempty" jsonschema:"Paths to analyze. Defaults to current directory if empty."`
	Format string   `json:"format,omitempty" jsonschema:"Output format: toon (default), json, or markdown."`
}

// ComplexityInput adds complexity-specific options.
type ComplexityInput struct {
	AnalyzeInput
	CyclomaticThreshold int  `json:"cyclomatic_threshold,omitempty" jsonschema:"Cyclomatic complexity threshold for warnings. Default 10."`
	CognitiveThreshold  int  `json:"cognitive_threshold,omitempty" jsonschema:"Cognitive complexity threshold for warnings. Default 15."`
	FunctionsOnly       bool `json:"functions_only,omitempty" jsonschema:"Show only function-level metrics, omit file summaries."`
}

// SATDInput adds SATD-specific options.
type SATDInput struct {
	AnalyzeInput
	IncludeTests bool     `json:"include_tests,omitempty" jsonschema:"Include test files in analysis."`
	StrictMode   bool     `json:"strict_mode,omitempty" jsonschema:"Only match explicit markers with colons (e.g., TODO:)."`
	Patterns     []string `json:"patterns,omitempty" jsonschema:"Additional patterns to detect beyond defaults."`
}

// DeadcodeInput adds deadcode-specific options.
type DeadcodeInput struct {
	AnalyzeInput
	Confidence float64 `json:"confidence,omitempty" jsonschema:"Minimum confidence threshold (0.0-1.0). Default 0.8."`
}

// ChurnInput adds churn-specific options.
type ChurnInput struct {
	AnalyzeInput
	Days int `json:"days,omitempty" jsonschema:"Number of days of git history to analyze. Default 30."`
	Top  int `json:"top,omitempty" jsonschema:"Show top N files by churn. Default 20."`
}

// DuplicatesInput adds duplicate detection options.
type DuplicatesInput struct {
	AnalyzeInput
	MinLines  int     `json:"min_lines,omitempty" jsonschema:"Minimum lines for clone detection. Default 6."`
	Threshold float64 `json:"threshold,omitempty" jsonschema:"Similarity threshold (0.0-1.0). Default 0.8."`
}

// DefectInput adds PMAT defect prediction options.
type DefectInput struct {
	AnalyzeInput
	HighRiskOnly bool `json:"high_risk_only,omitempty" jsonschema:"Show only high-risk files."`
}

// ChangesInput adds JIT change risk analysis options.
type ChangesInput struct {
	AnalyzeInput
	Days         int  `json:"days,omitempty" jsonschema:"Number of days of git history to analyze. Default 30."`
	Top          int  `json:"top,omitempty" jsonschema:"Show top N commits by risk. Default 20."`
	HighRiskOnly bool `json:"high_risk_only,omitempty" jsonschema:"Show only high-risk commits."`
}

// TDGInput adds TDG-specific options.
type TDGInput struct {
	AnalyzeInput
	Hotspots      int  `json:"hotspots,omitempty" jsonschema:"Number of hotspots to show. Default 10."`
	ShowPenalties bool `json:"show_penalties,omitempty" jsonschema:"Show applied penalties in output."`
}

// GraphInput adds graph-specific options.
type GraphInput struct {
	AnalyzeInput
	Scope          string `json:"scope,omitempty" jsonschema:"Analysis scope: file, function, module, or package. Default module."`
	IncludeMetrics bool   `json:"include_metrics,omitempty" jsonschema:"Include PageRank and centrality metrics."`
}

// HotspotInput adds hotspot-specific options.
type HotspotInput struct {
	AnalyzeInput
	Days int `json:"days,omitempty" jsonschema:"Number of days of git history to analyze. Default 30."`
	Top  int `json:"top,omitempty" jsonschema:"Show top N files by hotspot score. Default 20."`
}

// TemporalCouplingInput adds temporal coupling options.
type TemporalCouplingInput struct {
	AnalyzeInput
	Days         int `json:"days,omitempty" jsonschema:"Number of days of git history to analyze. Default 30."`
	MinCochanges int `json:"min_cochanges,omitempty" jsonschema:"Minimum co-changes to consider files coupled. Default 3."`
	Top          int `json:"top,omitempty" jsonschema:"Show top N file pairs. Default 20."`
}

// OwnershipInput adds ownership-specific options.
type OwnershipInput struct {
	AnalyzeInput
	Top            int  `json:"top,omitempty" jsonschema:"Show top N files by ownership concentration. Default 20."`
	IncludeTrivial bool `json:"include_trivial,omitempty" jsonschema:"Include trivial lines in ownership calculation."`
}

// CohesionInput adds cohesion-specific options.
type CohesionInput struct {
	AnalyzeInput
	IncludeTests bool   `json:"include_tests,omitempty" jsonschema:"Include test files in analysis."`
	Sort         string `json:"sort,omitempty" jsonschema:"Sort by metric: lcom, wmc, cbo, or dit. Default lcom."`
	Top          int    `json:"top,omitempty" jsonschema:"Show top N classes. Default 20."`
}

// RepoMapInput adds repo map options.
type RepoMapInput struct {
	AnalyzeInput
	Top int `json:"top,omitempty" jsonschema:"Number of top symbols to include. Default 50."`
}

// SmellsInput adds architectural smell detection options.
type SmellsInput struct {
	AnalyzeInput
	HubThreshold          int     `json:"hub_threshold,omitempty" jsonschema:"Fan-in + fan-out threshold for hub detection. Default 20."`
	GodFanInThreshold     int     `json:"god_fan_in,omitempty" jsonschema:"Minimum fan-in for god component detection. Default 10."`
	GodFanOutThreshold    int     `json:"god_fan_out,omitempty" jsonschema:"Minimum fan-out for god component detection. Default 10."`
	InstabilityDifference float64 `json:"instability_diff,omitempty" jsonschema:"Max instability difference for unstable dependency detection. Default 0.4."`
}

// FlagsInput adds feature flag detection options.
type FlagsInput struct {
	AnalyzeInput
	Providers   []string `json:"providers,omitempty" jsonschema:"Filter by providers: launchdarkly, split, unleash, posthog, flipper. Default all."`
	IncludeGit  bool     `json:"include_git,omitempty" jsonschema:"Include git history for staleness analysis. Default true."`
	MinPriority string   `json:"min_priority,omitempty" jsonschema:"Filter by minimum priority: LOW, MEDIUM, HIGH, CRITICAL."`
	Top         int      `json:"top,omitempty" jsonschema:"Show top N flags by priority. Default all."`
}

// ScoreInput adds repository score options.
type ScoreInput struct {
	AnalyzeInput
}

// ContextInput adds context options.
type ContextInput struct {
	Focus  string   `json:"focus" jsonschema:"File path, glob pattern, basename, or symbol name to focus on (required)."`
	Paths  []string `json:"paths,omitempty" jsonschema:"Repository paths to search. Defaults to current directory."`
	Format string   `json:"format,omitempty" jsonschema:"Output format: toon (default), json, or markdown."`
}

// TrendInput adds trend analysis options.
type TrendInput struct {
	AnalyzeInput
	Since  string `json:"since,omitempty" jsonschema:"How far back to analyze (e.g., 3m, 6m, 1y, 2y). Default 1y."`
	Period string `json:"period,omitempty" jsonschema:"Sampling period: daily, weekly, monthly. Default monthly."`
	Snap   bool   `json:"snap,omitempty" jsonschema:"Snap to period boundaries (1st of month, Monday)."`
}

// Helper functions

func getPaths(input AnalyzeInput) []string {
	if len(input.Paths) == 0 {
		return []string{"."}
	}
	return input.Paths
}

func getFormat(input AnalyzeInput) output.Format {
	switch input.Format {
	case "json":
		return output.FormatJSON
	case "markdown", "md":
		return output.FormatMarkdown
	default:
		return output.FormatTOON
	}
}

func formatOutput(data any, format output.Format) (string, error) {
	switch format {
	case output.FormatJSON:
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return string(out), nil
	case output.FormatMarkdown:
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return "```\n" + string(out) + "\n```", nil
	default:
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return string(out), nil
	}
}

func toolResult(data any, format output.Format) (*mcp.CallToolResult, any, error) {
	text, err := formatOutput(data, format)
	if err != nil {
		return nil, nil, err
	}
	return &mcp.CallToolResult{
		Content: []mcp.Content{
			&mcp.TextContent{Text: text},
		},
	}, nil, nil
}

func toolError(msg string) (*mcp.CallToolResult, any, error) {
	return &mcp.CallToolResult{
		Content: []mcp.Content{
			&mcp.TextContent{Text: "Error: " + msg},
		},
		IsError: true,
	}, nil, nil
}

// Tool handlers

func handleAnalyzeComplexity(ctx context.Context, req *mcp.CallToolRequest, input ComplexityInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeComplexity(ctx, scanResult.Files, analysis.ComplexityOptions{})
	if err != nil {
		return toolError(err.Error())
	}

	if input.FunctionsOnly {
		var functions []complexity.FunctionResult
		for _, f := range result.Files {
			functions = append(functions, f.Functions...)
		}
		out := struct {
			Functions []complexity.FunctionResult `json:"functions" toon:"functions"`
			Summary   complexity.Summary          `json:"summary" toon:"summary"`
		}{functions, result.Summary}
		return toolResult(out, format)
	}

	return toolResult(result, format)
}

func handleAnalyzeSATD(ctx context.Context, req *mcp.CallToolRequest, input SATDInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	var customPatterns []analysis.PatternConfig
	for _, p := range input.Patterns {
		customPatterns = append(customPatterns, analysis.PatternConfig{
			Pattern:  p,
			Category: satd.CategoryRequirement,
			Severity: satd.SeverityMedium,
		})
	}

	svc := analysis.New()
	result, err := svc.AnalyzeSATD(ctx, scanResult.Files, analysis.SATDOptions{
		IncludeTests:   input.IncludeTests,
		StrictMode:     input.StrictMode,
		CustomPatterns: customPatterns,
	})
	if err != nil {
		return toolError(err.Error())
	}

	return toolResult(result, format)
}

func handleAnalyzeDeadcode(ctx context.Context, req *mcp.CallToolRequest, input DeadcodeInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	confidence := input.Confidence
	if confidence <= 0 {
		confidence = 0.8
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeDeadCode(ctx, scanResult.Files, analysis.DeadCodeOptions{
		Confidence: confidence,
	})
	if err != nil {
		return toolError(err.Error())
	}

	out := deadcode.NewReport()
	out.FromAnalysis(result)

	return toolResult(out, format)
}

func handleAnalyzeChurn(ctx context.Context, req *mcp.CallToolRequest, input ChurnInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	days := input.Days
	if days <= 0 {
		days = 30
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	svc := analysis.New()
	result, err := svc.AnalyzeChurn(ctx, scanResult.RepoRoot, analysis.ChurnOptions{
		Days: days,
	})
	if err != nil {
		return toolError(err.Error())
	}

	top := input.Top
	if top > 0 && len(result.Files) > top {
		result.Files = result.Files[:top]
	}

	return toolResult(result, format)
}

func handleAnalyzeDuplicates(ctx context.Context, req *mcp.CallToolRequest, input DuplicatesInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	minLines := input.MinLines
	if minLines <= 0 {
		minLines = 6
	}
	threshold := input.Threshold
	if threshold <= 0 {
		threshold = 0.8
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeDuplicates(ctx, scanResult.Files, analysis.DuplicatesOptions{
		MinLines:            minLines,
		SimilarityThreshold: threshold,
	})
	if err != nil {
		return toolError(err.Error())
	}

	report := result.ToReport()
	return toolResult(report, format)
}

func handleAnalyzeDefect(ctx context.Context, req *mcp.CallToolRequest, input DefectInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeDefects(ctx, scanResult.RepoRoot, scanResult.Files, analysis.DefectOptions{})
	if err != nil {
		return toolError(err.Error())
	}

	sort.Slice(result.Files, func(i, j int) bool {
		return result.Files[i].Probability > result.Files[j].Probability
	})

	if input.HighRiskOnly {
		var filtered []defect.Score
		for _, ds := range result.Files {
			if ds.RiskLevel == defect.RiskHigh {
				filtered = append(filtered, ds)
			}
		}
		result.Files = filtered
	}

	report := result.ToReport()
	return toolResult(report, format)
}

func handleAnalyzeChanges(ctx context.Context, req *mcp.CallToolRequest, input ChangesInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	days := input.Days
	if days <= 0 {
		days = 30
	}
	top := input.Top
	if top <= 0 {
		top = 20
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	svc := analysis.New()
	result, err := svc.AnalyzeChanges(ctx, scanResult.RepoRoot, analysis.ChangesOptions{
		Days: days,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if result.Summary.TotalCommits == 0 {
		return toolError("no commits found in the specified time period")
	}

	if input.HighRiskOnly {
		var filtered []changes.CommitRisk
		for _, cr := range result.Commits {
			if cr.RiskLevel == changes.RiskLevelHigh {
				filtered = append(filtered, cr)
			}
		}
		result.Commits = filtered
	}

	if len(result.Commits) > top {
		result.Commits = result.Commits[:top]
	}

	return toolResult(result, format)
}

func handleAnalyzeTDG(ctx context.Context, req *mcp.CallToolRequest, input TDGInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	hotspots := input.Hotspots
	if hotspots <= 0 {
		hotspots = 10
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	project, err := svc.AnalyzeTDG(ctx, scanResult.Files)
	if err != nil {
		return toolError(err.Error())
	}

	report := project.ToReport(hotspots)

	if input.ShowPenalties {
		type HotspotWithPenalties struct {
			tdg.Hotspot
			Penalties []tdg.PenaltyAttribution `json:"penalties,omitempty" toon:"penalties,omitempty"`
		}
		type ReportWithPenalties struct {
			Summary  tdg.Summary            `json:"summary" toon:"summary"`
			Hotspots []HotspotWithPenalties `json:"hotspots" toon:"hotspots"`
		}

		penaltyMap := make(map[string][]tdg.PenaltyAttribution)
		for _, f := range project.Files {
			if len(f.PenaltiesApplied) > 0 {
				penaltyMap[f.FilePath] = f.PenaltiesApplied
			}
		}

		extendedHotspots := make([]HotspotWithPenalties, len(report.Hotspots))
		for i, h := range report.Hotspots {
			extendedHotspots[i] = HotspotWithPenalties{
				Hotspot:   h,
				Penalties: penaltyMap[h.Path],
			}
		}

		extendedReport := ReportWithPenalties{
			Summary:  report.Summary,
			Hotspots: extendedHotspots,
		}
		return toolResult(extendedReport, format)
	}

	return toolResult(report, format)
}

func handleAnalyzeGraph(ctx context.Context, req *mcp.CallToolRequest, input GraphInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	scope := input.Scope
	if scope == "" {
		scope = "module"
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	depGraph, metrics, err := svc.AnalyzeGraph(ctx, scanResult.Files, analysis.GraphOptions{
		Scope:          graph.Scope(scope),
		IncludeMetrics: input.IncludeMetrics,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if input.IncludeMetrics {
		result := struct {
			Graph   *graph.DependencyGraph `json:"graph" toon:"graph"`
			Metrics *graph.Metrics         `json:"metrics" toon:"metrics"`
		}{depGraph, metrics}
		return toolResult(result, format)
	}

	return toolResult(depGraph, format)
}

func handleAnalyzeHotspot(ctx context.Context, req *mcp.CallToolRequest, input HotspotInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	days := input.Days
	if days <= 0 {
		days = 30
	}
	top := input.Top
	if top <= 0 {
		top = 20
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeHotspots(ctx, scanResult.RepoRoot, scanResult.Files, analysis.HotspotOptions{
		Days: days,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if len(result.Files) > top {
		result.Files = result.Files[:top]
	}

	return toolResult(result, format)
}

func handleAnalyzeTemporalCoupling(ctx context.Context, req *mcp.CallToolRequest, input TemporalCouplingInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	days := input.Days
	if days <= 0 {
		days = 30
	}
	minCochanges := input.MinCochanges
	if minCochanges <= 0 {
		minCochanges = 3
	}
	top := input.Top
	if top <= 0 {
		top = 20
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	svc := analysis.New()
	result, err := svc.AnalyzeTemporalCoupling(ctx, scanResult.RepoRoot, analysis.TemporalCouplingOptions{
		Days:         days,
		MinCochanges: minCochanges,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if len(result.Couplings) > top {
		result.Couplings = result.Couplings[:top]
	}

	return toolResult(result, format)
}

func handleAnalyzeOwnership(ctx context.Context, req *mcp.CallToolRequest, input OwnershipInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	top := input.Top
	if top <= 0 {
		top = 20
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeOwnership(ctx, scanResult.RepoRoot, scanResult.Files, analysis.OwnershipOptions{
		IncludeTrivial: input.IncludeTrivial,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if len(result.Files) > top {
		result.Files = result.Files[:top]
	}

	return toolResult(result, format)
}

func handleAnalyzeCohesion(ctx context.Context, req *mcp.CallToolRequest, input CohesionInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	top := input.Top
	if top <= 0 {
		top = 20
	}
	sortBy := input.Sort
	if sortBy == "" {
		sortBy = "lcom"
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeCohesion(ctx, scanResult.Files, analysis.CohesionOptions{
		IncludeTests: input.IncludeTests,
	})
	if err != nil {
		return toolError(err.Error())
	}

	switch sortBy {
	case "wmc":
		result.SortByWMC()
	case "cbo":
		result.SortByCBO()
	case "dit":
		result.SortByDIT()
	default:
		result.SortByLCOM()
	}

	if len(result.Classes) > top {
		result.Classes = result.Classes[:top]
	}

	return toolResult(result, format)
}

func handleAnalyzeRepoMap(ctx context.Context, req *mcp.CallToolRequest, input RepoMapInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	top := input.Top
	if top <= 0 {
		top = 50
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	rm, err := svc.AnalyzeRepoMap(ctx, scanResult.Files, analysis.RepoMapOptions{Top: top})
	if err != nil {
		return toolError(err.Error())
	}

	topSymbols := rm.TopN(top)

	result := struct {
		Symbols []repomap.Symbol `json:"symbols" toon:"symbols"`
		Summary repomap.Summary  `json:"summary" toon:"summary"`
	}{
		Symbols: topSymbols,
		Summary: rm.Summary,
	}

	return toolResult(result, format)
}

func handleAnalyzeSmells(ctx context.Context, req *mcp.CallToolRequest, input SmellsInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	svc := analysis.New()
	result, err := svc.AnalyzeSmells(ctx, scanResult.Files, analysis.SmellOptions{
		HubThreshold:          input.HubThreshold,
		GodFanInThreshold:     input.GodFanInThreshold,
		GodFanOutThreshold:    input.GodFanOutThreshold,
		InstabilityDifference: input.InstabilityDifference,
	})
	if err != nil {
		return toolError(err.Error())
	}

	return toolResult(result, format)
}

func handleAnalyzeFlags(ctx context.Context, req *mcp.CallToolRequest, input FlagsInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	// Default include_git to true if not explicitly set
	includeGit := true
	if input.MinPriority != "" && !input.IncludeGit {
		// Only disable if explicitly requested
		includeGit = input.IncludeGit
	}

	svc := analysis.New()
	result, err := svc.AnalyzeFeatureFlags(ctx, scanResult.Files, analysis.FeatureFlagOptions{
		Providers:  input.Providers,
		IncludeGit: includeGit,
	})
	if err != nil {
		return toolError(err.Error())
	}

	// Filter by minimum priority if specified
	if input.MinPriority != "" {
		priorityOrder := map[string]int{
			"LOW":      1,
			"MEDIUM":   2,
			"HIGH":     3,
			"CRITICAL": 4,
		}
		minOrder := priorityOrder[input.MinPriority]
		if minOrder > 0 {
			var filtered []featureflags.FlagAnalysis
			for _, f := range result.Flags {
				if priorityOrder[string(f.Priority.Level)] >= minOrder {
					filtered = append(filtered, f)
				}
			}
			result.Flags = filtered
		}
	}

	// Limit to top N if specified
	if input.Top > 0 && len(result.Flags) > input.Top {
		result.Flags = result.Flags[:input.Top]
	}

	return toolResult(result, format)
}

func handleAnalyzeScore(ctx context.Context, req *mcp.CallToolRequest, input ScoreInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	cfg, err := config.LoadOrDefault()
	if err != nil {
		return toolError(err.Error())
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPaths(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(scanResult.Files) == 0 {
		return toolError("no source files found")
	}

	// Build weights and thresholds from config
	effectiveWeights := cfg.Score.EffectiveWeights()
	weights := score.Weights{
		Complexity:  effectiveWeights.Complexity,
		Duplication: effectiveWeights.Duplication,
		SATD:        effectiveWeights.SATD,
		TDG:         effectiveWeights.TDG,
		Coupling:    effectiveWeights.Coupling,
		Smells:      effectiveWeights.Smells,
		Cohesion:    effectiveWeights.Cohesion,
	}

	thresholds := score.Thresholds{
		Score:       cfg.Score.Thresholds.Score,
		Complexity:  cfg.Score.Thresholds.Complexity,
		Duplication: cfg.Score.Thresholds.Duplication,
		SATD:        cfg.Score.Thresholds.SATD,
		TDG:         cfg.Score.Thresholds.TDG,
		Coupling:    cfg.Score.Thresholds.Coupling,
		Smells:      cfg.Score.Thresholds.Smells,
		Cohesion:    cfg.Score.Thresholds.Cohesion,
	}

	analyzer := score.New(
		score.WithWeights(weights),
		score.WithThresholds(thresholds),
		score.WithChurnDays(cfg.Analysis.ChurnDays),
		score.WithMaxFileSize(cfg.Analysis.MaxFileSize),
	)
	result, err := analyzer.Analyze(ctx, scanResult.Files, source.NewFilesystem(), "")
	if err != nil {
		return toolError(err.Error())
	}

	return toolResult(result, format)
}

func handleAnalyzeTrend(ctx context.Context, req *mcp.CallToolRequest, input TrendInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	// Parse since duration (default 1y)
	sinceStr := input.Since
	if sinceStr == "" {
		sinceStr = "1y"
	}
	since, err := score.ParseSince(sinceStr)
	if err != nil {
		return toolError("invalid since value: " + err.Error())
	}

	// Validate period (default monthly)
	period := input.Period
	if period == "" {
		period = "monthly"
	}
	switch period {
	case "daily", "weekly", "monthly":
		// OK
	default:
		return toolError("invalid period: " + period + " (use daily, weekly, or monthly)")
	}

	scanner := scannerSvc.New()
	scanResult, err := scanner.ScanPathsForGit(paths, true)
	if err != nil {
		return toolError(err.Error())
	}

	// Create trend analyzer
	trendAnalyzer := score.NewTrendAnalyzer(
		score.WithTrendPeriod(period),
		score.WithTrendSince(since),
		score.WithTrendSnap(input.Snap),
	)

	result, err := trendAnalyzer.AnalyzeTrend(ctx, scanResult.RepoRoot)
	if err != nil {
		return toolError(err.Error())
	}

	return toolResult(result, format)
}

func handleGetContext(ctx context.Context, req *mcp.CallToolRequest, input ContextInput) (*mcp.CallToolResult, any, error) {
	if input.Focus == "" {
		return toolError("focus parameter is required")
	}

	paths := input.Paths
	if len(paths) == 0 {
		paths = []string{"."}
	}
	format := output.FormatTOON
	switch input.Format {
	case "json":
		format = output.FormatJSON
	case "markdown", "md":
		format = output.FormatMarkdown
	}

	baseDir := paths[0]
	svc := analysis.New()

	// Try without repo map first (exact path, glob, basename)
	result, err := svc.FocusedContext(ctx, analysis.FocusedContextOptions{
		Focus:   input.Focus,
		BaseDir: baseDir,
	})

	// If not found, try with repo map for symbol lookup
	if errors.Is(err, locator.ErrNotFound) {
		scanner := scannerSvc.New()
		scanResult, scanErr := scanner.ScanPaths(paths)
		if scanErr == nil && len(scanResult.Files) > 0 {
			repoMapResult, _ := svc.AnalyzeRepoMap(ctx, scanResult.Files, analysis.RepoMapOptions{})
			if repoMapResult != nil {
				result, err = svc.FocusedContext(ctx, analysis.FocusedContextOptions{
					Focus:   input.Focus,
					BaseDir: baseDir,
					RepoMap: repoMapResult,
				})
			}
		}
	}

	// Handle ambiguous match specially - return candidates as structured error
	if err != nil && result != nil && len(result.Candidates) > 0 {
		candidates := struct {
			Error      string                      `json:"error" toon:"error"`
			Candidates []analysis.FocusedCandidate `json:"candidates" toon:"candidates"`
		}{
			Error:      "ambiguous match: multiple files or symbols found",
			Candidates: result.Candidates,
		}
		text, _ := formatOutput(candidates, format)
		return &mcp.CallToolResult{
			Content: []mcp.Content{
				&mcp.TextContent{Text: text},
			},
			IsError: true,
		}, nil, nil
	}

	if err != nil {
		return toolError(err.Error())
	}

	return toolResult(result, format)
}
