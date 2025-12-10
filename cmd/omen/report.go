package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/report"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/churn"
	"github.com/panbanda/omen/pkg/analyzer/cohesion"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/featureflags"
	"github.com/panbanda/omen/pkg/analyzer/hotspot"
	"github.com/panbanda/omen/pkg/analyzer/ownership"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/panbanda/omen/pkg/source"
	"github.com/sourcegraph/conc"
	"github.com/spf13/cobra"
)

var (
	reportOutputDir    string
	reportSince        string
	reportDataDir      string
	reportOutputFile   string
	reportSkipValidate bool
	reportPort         int
)

var reportCmd = &cobra.Command{
	Use:   "report",
	Short: "Generate and manage health reports",
	Long: `Generate interactive HTML health reports with optional LLM insights.

The report workflow consists of:
  1. generate - Run all analyzers and output JSON data files
  2. validate - Validate data and insight files against schemas
  3. render   - Combine data + insights into self-contained HTML
  4. serve    - Serve rendered HTML with live re-render on request`,
}

var reportGenerateCmd = &cobra.Command{
	Use:   "generate [paths...]",
	Short: "Run all analyzers and output JSON data files",
	Long: `Runs all analyzers concurrently and outputs JSON data files to a directory.

The generated data files can be analyzed by an LLM to produce insight files,
then rendered into an interactive HTML report.`,
	RunE: runReportGenerate,
}

var reportValidateCmd = &cobra.Command{
	Use:   "validate",
	Short: "Validate data files and insights against schemas",
	Long: `Validates all data files exist and parse as valid JSON.
If an insights/ directory exists, validates each insight file against its schema.`,
	RunE: runReportValidate,
}

var reportRenderCmd = &cobra.Command{
	Use:   "render",
	Short: "Combine data + insights into self-contained HTML",
	Long: `Reads JSON files from the data directory and outputs a self-contained HTML report.
If an insights/ directory exists, includes LLM commentary sections.`,
	RunE: runReportRender,
}

var reportServeCmd = &cobra.Command{
	Use:   "serve",
	Short: "Serve rendered HTML with live re-render on request",
	Long: `Serves the rendered HTML report on the specified port.
Re-renders HTML on each request, allowing live iteration on insight files.`,
	RunE: runReportServe,
}

func init() {
	// Generate command flags
	reportGenerateCmd.Flags().StringVarP(&reportOutputDir, "output", "o", "", "Output directory (default: ./omen-report-<date>/)")
	reportGenerateCmd.Flags().StringVar(&reportSince, "since", "1y", "Time period for historical analysis (3m, 6m, 1y, 2y, all)")

	// Validate command flags
	reportValidateCmd.Flags().StringVarP(&reportDataDir, "data", "d", "", "Input data directory")

	// Render command flags
	reportRenderCmd.Flags().StringVarP(&reportDataDir, "data", "d", "", "Input data directory")
	reportRenderCmd.Flags().StringVarP(&reportOutputFile, "output", "o", "omen-report.html", "Output HTML file")
	reportRenderCmd.Flags().BoolVar(&reportSkipValidate, "skip-validate", false, "Skip validation before rendering")

	// Serve command flags
	reportServeCmd.Flags().StringVarP(&reportDataDir, "data", "d", "", "Input data directory")
	reportServeCmd.Flags().IntVarP(&reportPort, "port", "p", 8080, "Port number")

	// Register subcommands
	reportCmd.AddCommand(reportGenerateCmd)
	reportCmd.AddCommand(reportValidateCmd)
	reportCmd.AddCommand(reportRenderCmd)
	reportCmd.AddCommand(reportServeCmd)

	// Register with root
	rootCmd.AddCommand(reportCmd)
}

func runReportGenerate(cmd *cobra.Command, args []string) error {
	paths := getPaths(args)

	// Determine output directory
	outputDir := reportOutputDir
	if outputDir == "" {
		outputDir = fmt.Sprintf("omen-report-%s", time.Now().Format("2006-01-02"))
	}

	// Create output directory
	if err := os.MkdirAll(outputDir, 0755); err != nil {
		return fmt.Errorf("failed to create output directory: %w", err)
	}

	// Parse since duration
	days, err := parseSinceToDays(reportSince)
	if err != nil {
		return fmt.Errorf("invalid --since value: %w", err)
	}

	sinceDuration, err := score.ParseSince(reportSince)
	if err != nil {
		return fmt.Errorf("invalid --since value: %w", err)
	}

	// Get files for analysis
	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return fmt.Errorf("failed to scan paths: %w", err)
	}

	// Run analyzers concurrently with progress tracking
	const numAnalyzers = 11
	tracker := progress.NewTracker("Running analyzers", numAnalyzers)

	wg := conc.NewWaitGroup()
	var analysisResults reportAnalysisData
	var analysisErrors []error
	var errMu sync.Mutex

	// Score analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := score.New(score.WithChurnDays(days))
		result, err := analyzer.Analyze(context.Background(), scanResult.Files, source.NewFilesystem(), "")
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("score: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Score = result
	})

	// Complexity analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := complexity.New()
		defer analyzer.Close()
		result, err := analyzer.Analyze(context.Background(), scanResult.Files, source.NewFilesystem())
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("complexity: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Complexity = result
	})

	// Hotspot analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := hotspot.New(hotspot.WithChurnDays(days))
		defer analyzer.Close()
		result, err := analyzer.Analyze(context.Background(), paths[0], nil)
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("hotspot: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Hotspots = result
	})

	// Churn analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := churn.New(churn.WithDays(days))
		result, err := analyzer.Analyze(context.Background(), paths[0], nil)
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("churn: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Churn = result
	})

	// Ownership analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := ownership.New()
		result, err := analyzer.Analyze(context.Background(), paths[0], nil)
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("ownership: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Ownership = result
	})

	// SATD analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := satd.New()
		defer analyzer.Close()
		result, err := analyzer.Analyze(context.Background(), scanResult.Files, source.NewFilesystem())
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("satd: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.SATD = result
	})

	// Duplicates analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := duplicates.New()
		defer analyzer.Close()
		result, err := analyzer.Analyze(context.Background(), scanResult.Files, source.NewFilesystem())
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("duplicates: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Duplicates = result
	})

	// Feature flags analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer, err := featureflags.New()
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("featureflags init: %w", err))
			errMu.Unlock()
			return
		}
		defer analyzer.Close()
		result, err := analyzer.Analyze(context.Background(), scanResult.Files)
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("featureflags: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Flags = result
	})

	// Smells analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := smells.New()
		result, err := analyzer.Analyze(context.Background(), scanResult.Files)
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("smells: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Smells = result
	})

	// Cohesion analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := cohesion.New()
		defer analyzer.Close()
		result, err := analyzer.Analyze(context.Background(), scanResult.Files, source.NewFilesystem())
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("cohesion: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Cohesion = result
	})

	// Trend analyzer
	wg.Go(func() {
		defer tracker.Tick()
		analyzer := score.NewTrendAnalyzer(
			score.WithTrendSince(sinceDuration),
			score.WithTrendPeriod("monthly"),
			score.WithTrendChurnDays(days),
		)
		result, err := analyzer.AnalyzeTrend(context.Background(), paths[0])
		if err != nil {
			errMu.Lock()
			analysisErrors = append(analysisErrors, fmt.Errorf("trend: %w", err))
			errMu.Unlock()
			return
		}
		analysisResults.Trend = result
	})

	wg.Wait()
	tracker.FinishSuccess()

	// Check for errors
	if len(analysisErrors) > 0 {
		for _, e := range analysisErrors {
			fmt.Fprintf(os.Stderr, "Warning: %v\n", e)
		}
	}

	// Write metadata
	repoName := getRepoName(paths[0])
	meta := report.Metadata{
		Repository:  repoName,
		GeneratedAt: time.Now().UTC(),
		Since:       reportSince,
		OmenVersion: version,
		Paths:       paths,
	}
	if err := writeJSON(filepath.Join(outputDir, "metadata.json"), meta); err != nil {
		return fmt.Errorf("failed to write metadata: %w", err)
	}

	// Write data files
	if analysisResults.Score != nil {
		if err := writeJSON(filepath.Join(outputDir, "score.json"), analysisResults.Score); err != nil {
			return fmt.Errorf("failed to write score: %w", err)
		}
	}
	if analysisResults.Complexity != nil {
		if err := writeJSON(filepath.Join(outputDir, "complexity.json"), analysisResults.Complexity); err != nil {
			return fmt.Errorf("failed to write complexity: %w", err)
		}
	}
	if analysisResults.Hotspots != nil {
		if err := writeJSON(filepath.Join(outputDir, "hotspots.json"), analysisResults.Hotspots); err != nil {
			return fmt.Errorf("failed to write hotspots: %w", err)
		}
	}
	if analysisResults.Churn != nil {
		if err := writeJSON(filepath.Join(outputDir, "churn.json"), analysisResults.Churn); err != nil {
			return fmt.Errorf("failed to write churn: %w", err)
		}
	}
	if analysisResults.Ownership != nil {
		if err := writeJSON(filepath.Join(outputDir, "ownership.json"), analysisResults.Ownership); err != nil {
			return fmt.Errorf("failed to write ownership: %w", err)
		}
	}
	if analysisResults.SATD != nil {
		if err := writeJSON(filepath.Join(outputDir, "satd.json"), analysisResults.SATD); err != nil {
			return fmt.Errorf("failed to write satd: %w", err)
		}
	}
	if analysisResults.Duplicates != nil {
		if err := writeJSON(filepath.Join(outputDir, "duplicates.json"), analysisResults.Duplicates); err != nil {
			return fmt.Errorf("failed to write duplicates: %w", err)
		}
	}
	if analysisResults.Flags != nil {
		if err := writeJSON(filepath.Join(outputDir, "flags.json"), analysisResults.Flags); err != nil {
			return fmt.Errorf("failed to write flags: %w", err)
		}
	}
	if analysisResults.Smells != nil {
		if err := writeJSON(filepath.Join(outputDir, "smells.json"), analysisResults.Smells); err != nil {
			return fmt.Errorf("failed to write smells: %w", err)
		}
	}
	if analysisResults.Cohesion != nil {
		if err := writeJSON(filepath.Join(outputDir, "cohesion.json"), analysisResults.Cohesion); err != nil {
			return fmt.Errorf("failed to write cohesion: %w", err)
		}
	}
	if analysisResults.Trend != nil {
		if err := writeJSON(filepath.Join(outputDir, "trend.json"), analysisResults.Trend); err != nil {
			return fmt.Errorf("failed to write trend: %w", err)
		}
	}

	fmt.Printf("Report data generated: %s\n", outputDir)
	return nil
}

func runReportValidate(cmd *cobra.Command, args []string) error {
	dataDir := reportDataDir
	if dataDir == "" {
		return fmt.Errorf("--data flag is required")
	}

	// Required data files
	requiredFiles := []string{
		"metadata.json",
		"score.json",
		"complexity.json",
		"satd.json",
		"duplicates.json",
		"smells.json",
		"cohesion.json",
		"flags.json",
	}

	var errors []string

	// Check each required file exists and is valid JSON
	for _, file := range requiredFiles {
		path := filepath.Join(dataDir, file)
		data, err := os.ReadFile(path)
		if err != nil {
			if os.IsNotExist(err) {
				errors = append(errors, fmt.Sprintf("%s: file not found", file))
			} else {
				errors = append(errors, fmt.Sprintf("%s: %v", file, err))
			}
			continue
		}

		// Validate JSON syntax
		var js json.RawMessage
		if err := json.Unmarshal(data, &js); err != nil {
			errors = append(errors, fmt.Sprintf("%s: invalid JSON: %v", file, err))
		}
	}

	// Check for optional insight files
	insightsDir := filepath.Join(dataDir, "insights")
	if _, err := os.Stat(insightsDir); err == nil {
		insightFiles := []string{
			"summary.json",
			"trends.json",
			"components.json",
			"patterns.json",
			"hotspots.json",
			"ownership.json",
			"satd.json",
			"churn.json",
			"duplication.json",
		}

		for _, file := range insightFiles {
			path := filepath.Join(insightsDir, file)
			if data, err := os.ReadFile(path); err == nil {
				var js json.RawMessage
				if err := json.Unmarshal(data, &js); err != nil {
					errors = append(errors, fmt.Sprintf("insights/%s: invalid JSON: %v", file, err))
				}
			}
			// Missing insight files are not errors (they're optional)
		}
	}

	if len(errors) > 0 {
		for _, e := range errors {
			fmt.Fprintf(os.Stderr, "Validation error: %s\n", e)
		}
		return fmt.Errorf("validation failed with %d error(s)", len(errors))
	}

	fmt.Println("Validation passed")
	return nil
}

func runReportRender(cmd *cobra.Command, args []string) error {
	dataDir := reportDataDir
	if dataDir == "" {
		return fmt.Errorf("--data flag is required")
	}

	outputPath := reportOutputFile
	if outputPath == "" {
		outputPath = "omen-report.html"
	}

	// Run validation first unless skipped
	if !reportSkipValidate {
		if err := runReportValidate(cmd, args); err != nil {
			return fmt.Errorf("validation failed: %w", err)
		}
	}

	// Create renderer
	renderer, err := report.NewRenderer()
	if err != nil {
		return fmt.Errorf("failed to create renderer: %w", err)
	}

	// Render to file
	if err := renderer.RenderToFile(dataDir, outputPath); err != nil {
		return fmt.Errorf("failed to render report: %w", err)
	}

	fmt.Printf("Report rendered: %s\n", outputPath)
	return nil
}

func runReportServe(cmd *cobra.Command, args []string) error {
	dataDir := reportDataDir
	if dataDir == "" {
		return fmt.Errorf("--data flag is required")
	}

	// Validate on startup
	if err := runReportValidate(cmd, args); err != nil {
		return fmt.Errorf("validation failed: %w", err)
	}

	// Create renderer
	renderer, err := report.NewRenderer()
	if err != nil {
		return fmt.Errorf("failed to create renderer: %w", err)
	}

	// Create HTTP handler that re-renders on each request
	http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		if err := renderer.Render(dataDir, w); err != nil {
			http.Error(w, fmt.Sprintf("render error: %v", err), http.StatusInternalServerError)
		}
	})

	addr := fmt.Sprintf(":%d", reportPort)
	fmt.Printf("Serving report at http://localhost%s\n", addr)
	fmt.Println("Press Ctrl+C to stop")
	return http.ListenAndServe(addr, nil)
}

// reportAnalysisData holds results from all analyzers
type reportAnalysisData struct {
	Score      *score.Result
	Complexity *complexity.Analysis
	Hotspots   *hotspot.Analysis
	Churn      *churn.Analysis
	Ownership  *ownership.Analysis
	SATD       *satd.Analysis
	Duplicates *duplicates.Analysis
	Flags      *featureflags.Analysis
	Smells     *smells.Analysis
	Cohesion   *cohesion.Analysis
	Trend      *score.TrendResult
}

func parseSinceToDays(since string) (int, error) {
	switch since {
	case "1m":
		return 30, nil
	case "3m":
		return 90, nil
	case "6m":
		return 180, nil
	case "1y":
		return 365, nil
	case "2y":
		return 730, nil
	case "all":
		return 3650, nil // 10 years
	default:
		return 0, fmt.Errorf("invalid since value: %s (valid: 1m, 3m, 6m, 1y, 2y, all)", since)
	}
}

func writeJSON(path string, data any) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()

	enc := json.NewEncoder(f)
	enc.SetIndent("", "  ")
	return enc.Encode(data)
}

// getRepoName attempts to get the repository name from git, falling back to directory name.
func getRepoName(path string) string {
	// Try to get repo name from git remote
	absPath, err := filepath.Abs(path)
	if err != nil {
		return filepath.Base(path)
	}

	// Try git remote origin URL
	cmd := exec.Command("git", "-C", absPath, "config", "--get", "remote.origin.url")
	out, err := cmd.Output()
	if err == nil {
		url := strings.TrimSpace(string(out))
		// Extract repo name from URL (handles both HTTPS and SSH)
		// e.g., "https://github.com/org/repo.git" -> "repo"
		// e.g., "git@github.com:org/repo.git" -> "repo"
		url = strings.TrimSuffix(url, ".git")
		if idx := strings.LastIndex(url, "/"); idx >= 0 {
			return url[idx+1:]
		}
		if idx := strings.LastIndex(url, ":"); idx >= 0 {
			return url[idx+1:]
		}
	}

	// Fall back to directory name
	return filepath.Base(absPath)
}
