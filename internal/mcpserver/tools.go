package mcpserver

import (
	"context"
	"path/filepath"
	"sort"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/panbanda/omen/internal/analyzer"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/models"
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
	IncludeHalstead     bool `json:"include_halstead,omitempty" jsonschema:"Include Halstead software science metrics."`
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

// DefectInput adds defect prediction options.
type DefectInput struct {
	AnalyzeInput
	HighRiskOnly bool `json:"high_risk_only,omitempty" jsonschema:"Show only high-risk files."`
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
	result, err := svc.AnalyzeComplexity(scanResult.Files, analysis.ComplexityOptions{
		IncludeHalstead: input.IncludeHalstead,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if input.FunctionsOnly {
		var functions []models.FunctionComplexity
		for _, f := range result.Files {
			functions = append(functions, f.Functions...)
		}
		out := struct {
			Functions []models.FunctionComplexity `json:"functions" toon:"functions"`
			Summary   models.ComplexitySummary    `json:"summary" toon:"summary"`
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
			Category: models.DebtRequirement,
			Severity: models.SeverityMedium,
		})
	}

	svc := analysis.New()
	result, err := svc.AnalyzeSATD(scanResult.Files, analysis.SATDOptions{
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
	result, err := svc.AnalyzeDeadCode(scanResult.Files, analysis.DeadCodeOptions{
		Confidence: confidence,
	})
	if err != nil {
		return toolError(err.Error())
	}

	out := models.NewDeadCodeResult()
	out.FromDeadCodeAnalysis(result)

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
	result, err := svc.AnalyzeChurn(scanResult.RepoRoot, analysis.ChurnOptions{
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
	result, err := svc.AnalyzeDuplicates(scanResult.Files, analysis.DuplicatesOptions{
		MinLines:            minLines,
		SimilarityThreshold: threshold,
	})
	if err != nil {
		return toolError(err.Error())
	}

	report := result.ToCloneReport()
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
	result, err := svc.AnalyzeDefects(scanResult.RepoRoot, scanResult.Files, analysis.DefectOptions{})
	if err != nil {
		return toolError(err.Error())
	}

	sort.Slice(result.Files, func(i, j int) bool {
		return result.Files[i].Probability > result.Files[j].Probability
	})

	if input.HighRiskOnly {
		var filtered []models.DefectScore
		for _, ds := range result.Files {
			if ds.RiskLevel == models.RiskHigh {
				filtered = append(filtered, ds)
			}
		}
		result.Files = filtered
	}

	report := result.ToDefectPredictionReport()
	return toolResult(report, format)
}

func handleAnalyzeTDG(ctx context.Context, req *mcp.CallToolRequest, input TDGInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	hotspots := input.Hotspots
	if hotspots <= 0 {
		hotspots = 10
	}

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return toolError(err.Error())
	}

	svc := analysis.New()
	project, err := svc.AnalyzeTDG(absPath)
	if err != nil {
		return toolError(err.Error())
	}

	report := project.ToTDGReport(hotspots)

	if input.ShowPenalties {
		type HotspotWithPenalties struct {
			models.TDGHotspot
			Penalties []models.PenaltyAttribution `json:"penalties,omitempty" toon:"penalties,omitempty"`
		}
		type ReportWithPenalties struct {
			Summary  models.TDGSummary      `json:"summary" toon:"summary"`
			Hotspots []HotspotWithPenalties `json:"hotspots" toon:"hotspots"`
		}

		penaltyMap := make(map[string][]models.PenaltyAttribution)
		for _, f := range project.Files {
			if len(f.PenaltiesApplied) > 0 {
				penaltyMap[f.FilePath] = f.PenaltiesApplied
			}
		}

		extendedHotspots := make([]HotspotWithPenalties, len(report.Hotspots))
		for i, h := range report.Hotspots {
			extendedHotspots[i] = HotspotWithPenalties{
				TDGHotspot: h,
				Penalties:  penaltyMap[h.Path],
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
	graph, metrics, err := svc.AnalyzeGraph(scanResult.Files, analysis.GraphOptions{
		Scope:          analyzer.GraphScope(scope),
		IncludeMetrics: input.IncludeMetrics,
	})
	if err != nil {
		return toolError(err.Error())
	}

	if input.IncludeMetrics {
		result := struct {
			Graph   *models.DependencyGraph `json:"graph" toon:"graph"`
			Metrics *models.GraphMetrics    `json:"metrics" toon:"metrics"`
		}{graph, metrics}
		return toolResult(result, format)
	}

	return toolResult(graph, format)
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
	result, err := svc.AnalyzeHotspots(scanResult.RepoRoot, scanResult.Files, analysis.HotspotOptions{
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
	result, err := svc.AnalyzeTemporalCoupling(scanResult.RepoRoot, analysis.TemporalCouplingOptions{
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
	result, err := svc.AnalyzeOwnership(scanResult.RepoRoot, scanResult.Files, analysis.OwnershipOptions{
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
	result, err := svc.AnalyzeCohesion(scanResult.Files, analysis.CohesionOptions{
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
	rm, err := svc.AnalyzeRepoMap(scanResult.Files, analysis.RepoMapOptions{Top: top})
	if err != nil {
		return toolError(err.Error())
	}

	topSymbols := rm.TopN(top)

	result := struct {
		Symbols []models.Symbol       `json:"symbols" toon:"symbols"`
		Summary models.RepoMapSummary `json:"summary" toon:"summary"`
	}{
		Symbols: topSymbols,
		Summary: rm.Summary,
	}

	return toolResult(result, format)
}
