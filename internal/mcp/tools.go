package mcp

import (
	"context"
	"fmt"
	"path/filepath"
	"sort"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/panbanda/omen/internal/analyzer"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/scanner"
	"github.com/panbanda/omen/internal/vcs"
	"github.com/panbanda/omen/pkg/config"
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

func scanFiles(paths []string) ([]string, error) {
	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return nil, fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return nil, fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}
	return files, nil
}

func findGitRoot(path string) (string, error) {
	absPath, err := filepath.Abs(path)
	if err != nil {
		return "", fmt.Errorf("invalid path: %w", err)
	}

	// Try to open git repo starting from path
	_, err = vcs.DefaultOpener().PlainOpenWithDetect(absPath)
	if err != nil {
		return "", fmt.Errorf("not a git repository (or any parent): %w", err)
	}

	return absPath, nil
}

func formatOutput(data any, format output.Format) (string, error) {
	switch format {
	case output.FormatJSON:
		// For JSON, use toon output as it's similar structure
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return string(out), nil
	case output.FormatMarkdown:
		// For markdown, wrap in code block
		out, err := toon.Marshal(data, toon.WithIndent(2))
		if err != nil {
			return "", err
		}
		return "```\n" + string(out) + "\n```", nil
	default:
		// TOON format (default)
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

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	var cxOpts []analyzer.ComplexityOption
	if input.IncludeHalstead {
		cxOpts = append(cxOpts, analyzer.WithHalstead())
	}
	cxAnalyzer := analyzer.NewComplexityAnalyzer(cxOpts...)
	defer cxAnalyzer.Close()

	analysis, err := cxAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	if input.FunctionsOnly {
		// Extract just functions from all files
		var functions []models.FunctionComplexity
		for _, f := range analysis.Files {
			functions = append(functions, f.Functions...)
		}
		result := struct {
			Functions []models.FunctionComplexity `json:"functions" toon:"functions"`
			Summary   models.ComplexitySummary    `json:"summary" toon:"summary"`
		}{functions, analysis.Summary}
		return toolResult(result, format)
	}

	return toolResult(analysis, format)
}

func handleAnalyzeSATD(ctx context.Context, req *mcp.CallToolRequest, input SATDInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	var satdOpts []analyzer.SATDOption
	if !input.IncludeTests {
		satdOpts = append(satdOpts, analyzer.WithSATDSkipTests())
	}
	if input.StrictMode {
		satdOpts = append(satdOpts, analyzer.WithSATDStrictMode())
	}
	satdAnalyzer := analyzer.NewSATDAnalyzer(satdOpts...)

	// Add custom patterns if provided
	for _, pattern := range input.Patterns {
		// Add as medium severity requirement debt by default
		if err := satdAnalyzer.AddPattern(pattern, models.DebtRequirement, models.SeverityMedium); err != nil {
			return toolError(fmt.Sprintf("invalid pattern %q: %v", pattern, err))
		}
	}

	analysis, err := satdAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	return toolResult(analysis, format)
}

func handleAnalyzeDeadcode(ctx context.Context, req *mcp.CallToolRequest, input DeadcodeInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	confidence := input.Confidence
	if confidence <= 0 {
		confidence = 0.8
	}

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	dcAnalyzer := analyzer.NewDeadCodeAnalyzer(analyzer.WithDeadCodeConfidence(confidence))
	defer dcAnalyzer.Close()

	analysis, err := dcAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	result := models.NewDeadCodeResult()
	result.FromDeadCodeAnalysis(analysis)

	return toolResult(result, format)
}

func handleAnalyzeChurn(ctx context.Context, req *mcp.CallToolRequest, input ChurnInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	days := input.Days
	if days <= 0 {
		days = 30
	}

	repoPath, err := findGitRoot(paths[0])
	if err != nil {
		return toolError(err.Error())
	}

	churnAnalyzer := analyzer.NewChurnAnalyzer(analyzer.WithChurnDays(days))
	analysis, err := churnAnalyzer.AnalyzeRepo(repoPath)
	if err != nil {
		return toolError(fmt.Sprintf("churn analysis failed: %v", err))
	}

	// Limit results if top is specified
	top := input.Top
	if top > 0 && len(analysis.Files) > top {
		analysis.Files = analysis.Files[:top]
	}

	return toolResult(analysis, format)
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

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	dupAnalyzer := analyzer.NewDuplicateAnalyzer(
		analyzer.WithDuplicateMinTokens(minLines*8), // Convert lines to approximate tokens
		analyzer.WithDuplicateSimilarityThreshold(threshold),
	)
	defer dupAnalyzer.Close()

	analysis, err := dupAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	report := analysis.ToCloneReport()
	return toolResult(report, format)
}

func handleAnalyzeDefect(ctx context.Context, req *mcp.CallToolRequest, input DefectInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	repoPath, err := findGitRoot(paths[0])
	if err != nil {
		return toolError(err.Error())
	}

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	cfg := config.LoadOrDefault()
	defectAnalyzer := analyzer.NewDefectAnalyzer(analyzer.WithDefectChurnDays(cfg.Analysis.ChurnDays))
	defer defectAnalyzer.Close()

	analysis, err := defectAnalyzer.AnalyzeProject(repoPath, files)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	// Sort by probability
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].Probability > analysis.Files[j].Probability
	})

	// Filter if high risk only
	if input.HighRiskOnly {
		var filtered []models.DefectScore
		for _, ds := range analysis.Files {
			if ds.RiskLevel == models.RiskHigh {
				filtered = append(filtered, ds)
			}
		}
		analysis.Files = filtered
	}

	report := analysis.ToDefectPredictionReport()
	return toolResult(report, format)
}

func handleAnalyzeTDG(ctx context.Context, req *mcp.CallToolRequest, input TDGInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	hotspots := input.Hotspots
	if hotspots <= 0 {
		hotspots = 10
	}

	tdgAnalyzer := analyzer.NewTdgAnalyzer()
	defer tdgAnalyzer.Close()

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return toolError(fmt.Sprintf("invalid path: %v", err))
	}

	project, err := tdgAnalyzer.AnalyzeProject(absPath)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	report := project.ToTDGReport(hotspots)

	if input.ShowPenalties {
		// Create extended report with penalties included
		type HotspotWithPenalties struct {
			models.TDGHotspot
			Penalties []models.PenaltyAttribution `json:"penalties,omitempty" toon:"penalties,omitempty"`
		}
		type ReportWithPenalties struct {
			Summary  models.TDGSummary      `json:"summary" toon:"summary"`
			Hotspots []HotspotWithPenalties `json:"hotspots" toon:"hotspots"`
		}

		// Build file path to TdgScore lookup
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

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	graphAnalyzer := analyzer.NewGraphAnalyzer(analyzer.WithGraphScope(analyzer.GraphScope(scope)))
	defer graphAnalyzer.Close()

	graph, err := graphAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("analysis failed: %v", err))
	}

	if input.IncludeMetrics {
		metrics := graphAnalyzer.CalculateMetrics(graph)
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

	repoPath, err := findGitRoot(paths[0])
	if err != nil {
		return toolError(err.Error())
	}

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	hotspotAnalyzer := analyzer.NewHotspotAnalyzer(analyzer.WithHotspotChurnDays(days))
	defer hotspotAnalyzer.Close()

	analysis, err := hotspotAnalyzer.AnalyzeProject(repoPath, files)
	if err != nil {
		return toolError(fmt.Sprintf("hotspot analysis failed: %v", err))
	}

	// Limit results
	if len(analysis.Files) > top {
		analysis.Files = analysis.Files[:top]
	}

	return toolResult(analysis, format)
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

	repoPath, err := findGitRoot(paths[0])
	if err != nil {
		return toolError(err.Error())
	}

	tcAnalyzer := analyzer.NewTemporalCouplingAnalyzer(days, minCochanges)
	defer tcAnalyzer.Close()

	analysis, err := tcAnalyzer.AnalyzeRepo(repoPath)
	if err != nil {
		return toolError(fmt.Sprintf("temporal coupling analysis failed: %v", err))
	}

	// Limit results
	if len(analysis.Couplings) > top {
		analysis.Couplings = analysis.Couplings[:top]
	}

	return toolResult(analysis, format)
}

func handleAnalyzeOwnership(ctx context.Context, req *mcp.CallToolRequest, input OwnershipInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	top := input.Top
	if top <= 0 {
		top = 20
	}

	repoPath, err := findGitRoot(paths[0])
	if err != nil {
		return toolError(err.Error())
	}

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	var ownOpts []analyzer.OwnershipOption
	if input.IncludeTrivial {
		ownOpts = append(ownOpts, analyzer.WithOwnershipIncludeTrivial())
	}
	ownAnalyzer := analyzer.NewOwnershipAnalyzer(ownOpts...)
	defer ownAnalyzer.Close()

	analysis, err := ownAnalyzer.AnalyzeRepo(repoPath, files)
	if err != nil {
		return toolError(fmt.Sprintf("ownership analysis failed: %v", err))
	}

	// Limit results
	if len(analysis.Files) > top {
		analysis.Files = analysis.Files[:top]
	}

	return toolResult(analysis, format)
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

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	var ckOpts []analyzer.CohesionOption
	if input.IncludeTests {
		ckOpts = append(ckOpts, analyzer.WithCohesionIncludeTestFiles())
	}
	ckAnalyzer := analyzer.NewCohesionAnalyzer(ckOpts...)
	defer ckAnalyzer.Close()

	analysis, err := ckAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("cohesion analysis failed: %v", err))
	}

	// Sort by requested metric
	switch sortBy {
	case "wmc":
		analysis.SortByWMC()
	case "cbo":
		analysis.SortByCBO()
	case "dit":
		analysis.SortByDIT()
	default:
		analysis.SortByLCOM()
	}

	// Limit results
	if len(analysis.Classes) > top {
		analysis.Classes = analysis.Classes[:top]
	}

	return toolResult(analysis, format)
}

func handleAnalyzeRepoMap(ctx context.Context, req *mcp.CallToolRequest, input RepoMapInput) (*mcp.CallToolResult, any, error) {
	paths := getPaths(input.AnalyzeInput)
	format := getFormat(input.AnalyzeInput)

	top := input.Top
	if top <= 0 {
		top = 50
	}

	files, err := scanFiles(paths)
	if err != nil {
		return toolError(err.Error())
	}

	if len(files) == 0 {
		return toolError("no source files found")
	}

	rmAnalyzer := analyzer.NewRepoMapAnalyzer()
	defer rmAnalyzer.Close()

	rm, err := rmAnalyzer.AnalyzeProject(files)
	if err != nil {
		return toolError(fmt.Sprintf("repo map analysis failed: %v", err))
	}

	// Get top N symbols
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
