package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"runtime/pprof"
	"sort"
	"strings"
	"time"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/mcpserver"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/changes"
	"github.com/panbanda/omen/pkg/analyzer/churn"
	"github.com/panbanda/omen/pkg/analyzer/cohesion"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/deadcode"
	"github.com/panbanda/omen/pkg/analyzer/defect"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/featureflags"
	"github.com/panbanda/omen/pkg/analyzer/graph"
	"github.com/panbanda/omen/pkg/analyzer/hotspot"
	"github.com/panbanda/omen/pkg/analyzer/ownership"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/score"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/panbanda/omen/pkg/analyzer/tdg"
	"github.com/panbanda/omen/pkg/analyzer/temporal"
	"github.com/panbanda/omen/pkg/config"
	"github.com/pelletier/go-toml"
	"github.com/urfave/cli/v2"
)

var (
	version = "dev"
	commit  = "none"    //nolint:unused // set via ldflags at build time
	date    = "unknown" //nolint:unused // set via ldflags at build time
)

// getPaths returns paths from positional args, defaulting to ["."]
// Filters out any flag-like arguments that weren't parsed due to POSIX flag ordering.
func getPaths(c *cli.Context) []string {
	args := c.Args().Slice()
	if len(args) == 0 {
		return []string{"."}
	}

	// Filter out unparsed flags (arguments starting with -)
	var paths []string
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if strings.HasPrefix(arg, "-") {
			// Skip this flag and its value if it's a known flag with a value
			if (arg == "-f" || arg == "--format" || arg == "-o" || arg == "--output") && i+1 < len(args) {
				i++ // Skip the next arg (the value)
			}
			continue
		}
		paths = append(paths, arg)
	}

	if len(paths) == 0 {
		return []string{"."}
	}
	return paths
}

// getTrailingFlag retrieves a string flag value, checking both parsed flags and unparsed trailing args.
// This handles POSIX-style flag ordering where flags after positional args aren't parsed.
// Parameters:
//   - c: CLI context
//   - name: long flag name (e.g., "format")
//   - shortName: short flag name (e.g., "f"), or empty string if none
//   - defaultValue: value to compare against for "changed" detection
func getTrailingFlag(c *cli.Context, name, shortName, defaultValue string) string {
	// First check if the flag was properly parsed and differs from default
	if val := c.String(name); val != "" && val != defaultValue {
		return val
	}

	// Check unparsed args for trailing flags
	args := c.Args().Slice()
	longFlag := "--" + name
	shortFlag := ""
	if shortName != "" {
		shortFlag = "-" + shortName
	}

	for i := 0; i < len(args); i++ {
		arg := args[i]

		// Handle "--flag value" or "-f value" style
		if (arg == longFlag || (shortFlag != "" && arg == shortFlag)) && i+1 < len(args) {
			return args[i+1]
		}

		// Handle "--flag=value" style
		if strings.HasPrefix(arg, longFlag+"=") {
			return strings.TrimPrefix(arg, longFlag+"=")
		}

		// Handle "-f=value" style
		if shortFlag != "" && strings.HasPrefix(arg, shortFlag+"=") {
			return strings.TrimPrefix(arg, shortFlag+"=")
		}
	}

	// Fall back to parsed value (may be default)
	return c.String(name)
}

// getFormat returns the format flag value, checking both parsed flags and unparsed trailing args.
func getFormat(c *cli.Context) string {
	return getTrailingFlag(c, "format", "f", "text")
}

// getOutputFile returns the output file path, checking both parsed flags and unparsed trailing args.
func getOutputFile(c *cli.Context) string {
	return getTrailingFlag(c, "output", "o", "")
}

// getSort returns the sort flag value, checking both parsed flags and unparsed trailing args.
func getSort(c *cli.Context, defaultValue string) string {
	return getTrailingFlag(c, "sort", "", defaultValue)
}

// validateDays validates the --days flag and returns an error if invalid.
func validateDays(days int) error {
	if days <= 0 {
		return fmt.Errorf("--days must be a positive integer (got %d)", days)
	}
	return nil
}

// outputFlags returns the common output-related flags for analyze commands.
func outputFlags() []cli.Flag {
	return []cli.Flag{
		&cli.StringFlag{
			Name:    "format",
			Aliases: []string{"f"},
			Value:   "text",
			Usage:   "Output format: text, json, markdown, toon",
		},
		&cli.StringFlag{
			Name:    "output",
			Aliases: []string{"o"},
			Usage:   "Write output to file",
		},
		&cli.BoolFlag{
			Name:  "no-cache",
			Usage: "Disable caching",
		},
	}
}

func main() {
	app := &cli.App{
		Name:                   "omen",
		Usage:                  "Multi-language code analysis CLI",
		Version:                version,
		Metadata:               make(map[string]interface{}),
		UseShortOptionHandling: true,
		Description: `Omen analyzes codebases for complexity, technical debt, dead code,
code duplication, defect prediction, and dependency graphs.

Supports: Go, Rust, Python, TypeScript, JavaScript, Java, C, C++, Ruby, PHP`,
		Flags: []cli.Flag{
			&cli.StringFlag{
				Name:    "config",
				Aliases: []string{"c"},
				Usage:   "Path to config file (TOML, YAML, or JSON)",
				EnvVars: []string{"OMEN_CONFIG"},
			},
			&cli.BoolFlag{
				Name:  "verbose",
				Usage: "Enable verbose output",
			},
			&cli.StringFlag{
				Name:  "pprof",
				Usage: "Enable pprof profiling and write to specified prefix (creates <prefix>.cpu.pprof and <prefix>.mem.pprof)",
			},
		},
		Before: func(c *cli.Context) error {
			if pprofPrefix := c.String("pprof"); pprofPrefix != "" {
				cpuFile, err := os.Create(pprofPrefix + ".cpu.pprof")
				if err != nil {
					return fmt.Errorf("failed to create CPU profile: %w", err)
				}
				if err := pprof.StartCPUProfile(cpuFile); err != nil {
					cpuFile.Close()
					return fmt.Errorf("failed to start CPU profile: %w", err)
				}
				// Store file handle for cleanup
				c.App.Metadata["pprofCPU"] = cpuFile
			}
			return nil
		},
		After: func(c *cli.Context) error {
			if pprofPrefix := c.String("pprof"); pprofPrefix != "" {
				// Stop CPU profile
				pprof.StopCPUProfile()
				if cpuFile, ok := c.App.Metadata["pprofCPU"].(*os.File); ok {
					cpuFile.Close()
					color.Green("CPU profile written to %s.cpu.pprof", pprofPrefix)
				}

				// Write memory profile
				memFile, err := os.Create(pprofPrefix + ".mem.pprof")
				if err != nil {
					return fmt.Errorf("failed to create memory profile: %w", err)
				}
				defer memFile.Close()

				runtime.GC() // Get up-to-date statistics
				if err := pprof.WriteHeapProfile(memFile); err != nil {
					return fmt.Errorf("failed to write memory profile: %w", err)
				}
				color.Green("Memory profile written to %s.mem.pprof", pprofPrefix)
			}
			return nil
		},
		Commands: []*cli.Command{
			initCmd(),
			configCmd(),
			analyzeCmd(),
			contextCmd(),
			mcpCmd(),
			scoreCmd(),
		},
	}

	if err := app.Run(os.Args); err != nil {
		color.Red("Error: %v", err)
		os.Exit(1)
	}
}

func complexityCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "cyclomatic-threshold",
			Value: 10,
			Usage: "Cyclomatic complexity warning threshold",
		},
		&cli.IntFlag{
			Name:  "cognitive-threshold",
			Value: 15,
			Usage: "Cognitive complexity warning threshold",
		},
		&cli.BoolFlag{
			Name:  "functions-only",
			Usage: "Show only function-level metrics",
		},
	)
	return &cli.Command{
		Name:      "complexity",
		Aliases:   []string{"cx"},
		Usage:     "Analyze cyclomatic and cognitive complexity",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runComplexityCmd,
	}
}

func runComplexityCmd(c *cli.Context) error {
	paths := getPaths(c)
	cycThreshold := c.Int("cyclomatic-threshold")
	cogThreshold := c.Int("cognitive-threshold")
	functionsOnly := c.Bool("functions-only")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Analyzing complexity...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeComplexity(scanResult.Files, analysis.ComplexityOptions{
		CyclomaticThreshold: cycThreshold,
		CognitiveThreshold:  cogThreshold,
		FunctionsOnly:       functionsOnly,
		OnProgress:          tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Build table rows for text/markdown output
	var rows [][]string
	var warnings []string

	for _, fc := range result.Files {
		if functionsOnly {
			for _, fn := range fc.Functions {
				cycColor := fmt.Sprintf("%d", fn.Metrics.Cyclomatic)
				cogColor := fmt.Sprintf("%d", fn.Metrics.Cognitive)

				if fn.Metrics.Cyclomatic > uint32(cycThreshold) {
					cycColor = color.RedString("%d", fn.Metrics.Cyclomatic)
					warnings = append(warnings, fmt.Sprintf("%s:%d %s - cyclomatic complexity %d exceeds threshold %d",
						fc.Path, fn.StartLine, fn.Name, fn.Metrics.Cyclomatic, cycThreshold))
				}
				if fn.Metrics.Cognitive > uint32(cogThreshold) {
					cogColor = color.RedString("%d", fn.Metrics.Cognitive)
					warnings = append(warnings, fmt.Sprintf("%s:%d %s - cognitive complexity %d exceeds threshold %d",
						fc.Path, fn.StartLine, fn.Name, fn.Metrics.Cognitive, cogThreshold))
				}

				rows = append(rows, []string{
					fc.Path,
					fn.Name,
					fmt.Sprintf("%d", fn.StartLine),
					cycColor,
					cogColor,
					fmt.Sprintf("%d", fn.Metrics.MaxNesting),
				})
			}
		} else {
			avgCyc := fmt.Sprintf("%.1f", fc.AvgCyclomatic)
			avgCog := fmt.Sprintf("%.1f", fc.AvgCognitive)

			if fc.AvgCyclomatic > float64(cycThreshold) {
				avgCyc = color.RedString("%.1f", fc.AvgCyclomatic)
			}
			if fc.AvgCognitive > float64(cogThreshold) {
				avgCog = color.RedString("%.1f", fc.AvgCognitive)
			}

			rows = append(rows, []string{
				fc.Path,
				fmt.Sprintf("%d", len(fc.Functions)),
				avgCyc,
				avgCog,
			})
		}
	}

	var headers []string
	if functionsOnly {
		headers = []string{"File", "Function", "Line", "Cyclomatic", "Cognitive", "Nesting"}
	} else {
		headers = []string{"File", "Functions", "Avg Cyclomatic", "Avg Cognitive"}
	}

	table := output.NewTable(
		"Complexity Analysis",
		headers,
		rows,
		[]string{
			fmt.Sprintf("Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("Functions: %d", result.Summary.TotalFunctions),
			fmt.Sprintf("Median Cyclomatic (P50): %d", result.Summary.P50Cyclomatic),
			fmt.Sprintf("Median Cognitive (P50): %d", result.Summary.P50Cognitive),
			fmt.Sprintf("90th Percentile Cyclomatic: %d", result.Summary.P90Cyclomatic),
			fmt.Sprintf("90th Percentile Cognitive: %d", result.Summary.P90Cognitive),
			fmt.Sprintf("Max Cyclomatic: %d", result.Summary.MaxCyclomatic),
			fmt.Sprintf("Max Cognitive: %d", result.Summary.MaxCognitive),
		},
		result,
	)

	if err := formatter.Output(table); err != nil {
		return err
	}

	if len(warnings) > 0 && formatter.Format() == output.FormatText {
		fmt.Println()
		color.Yellow("Warnings (%d):", len(warnings))
		for _, w := range warnings {
			fmt.Printf("  - %s\n", w)
		}
	}

	return nil
}

func satdCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.StringSliceFlag{
			Name:  "patterns",
			Usage: "Additional patterns to detect",
		},
		&cli.BoolFlag{
			Name:  "include-test",
			Usage: "Include test files in analysis",
		},
	)
	return &cli.Command{
		Name:      "satd",
		Aliases:   []string{"debt"},
		Usage:     "Detect self-admitted technical debt (TODO, FIXME, HACK)",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runSATDCmd,
	}
}

func runSATDCmd(c *cli.Context) error {
	paths := getPaths(c)
	patterns := c.StringSlice("patterns")
	includeTest := c.Bool("include-test")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	// Convert patterns to PatternConfig
	var customPatterns []analysis.PatternConfig
	for _, p := range patterns {
		customPatterns = append(customPatterns, analysis.PatternConfig{
			Pattern:  p,
			Category: satd.CategoryDesign,
			Severity: satd.SeverityMedium,
		})
	}

	tracker := progress.NewTracker("Detecting technical debt...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeSATD(scanResult.Files, analysis.SATDOptions{
		IncludeTests:   includeTest,
		CustomPatterns: customPatterns,
		OnProgress:     tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	var rows [][]string
	for _, item := range result.Items {
		sevColor := string(item.Severity)
		switch item.Severity {
		case satd.SeverityHigh:
			sevColor = color.RedString(string(item.Severity))
		case satd.SeverityMedium:
			sevColor = color.YellowString(string(item.Severity))
		case satd.SeverityLow:
			sevColor = color.GreenString(string(item.Severity))
		}

		rows = append(rows, []string{
			fmt.Sprintf("%s:%d", item.File, item.Line),
			item.Marker,
			sevColor,
			truncate(item.Description, 60),
		})
	}

	table := output.NewTable(
		"Self-Admitted Technical Debt",
		[]string{"Location", "Marker", "Severity", "Description"},
		rows,
		[]string{
			fmt.Sprintf("Total: %d", result.Summary.TotalItems),
			fmt.Sprintf("High: %d", result.Summary.BySeverity["high"]),
			fmt.Sprintf("Medium: %d", result.Summary.BySeverity["medium"]),
			fmt.Sprintf("Low: %d", result.Summary.BySeverity["low"]),
		},
		result,
	)

	return formatter.Output(table)
}

func truncate(s string, maxLen int) string {
	if len(s) <= maxLen {
		return s
	}
	return s[:maxLen-3] + "..."
}

func deadcodeCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.Float64Flag{
			Name:  "confidence",
			Value: 0.8,
			Usage: "Minimum confidence threshold (0.0-1.0)",
		},
	)
	return &cli.Command{
		Name:      "deadcode",
		Aliases:   []string{"dc"},
		Usage:     "Detect unused functions, variables, and unreachable code",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runDeadCodeCmd,
	}
}

func runDeadCodeCmd(c *cli.Context) error {
	paths := getPaths(c)
	confidence := c.Float64("confidence")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting dead code...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeDeadCode(scanResult.Files, analysis.DeadCodeOptions{
		Confidence: confidence,
		OnProgress: tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		pmatResult := deadcode.NewReport()
		pmatResult.FromAnalysis(result)
		return formatter.Output(pmatResult)
	}

	// Functions table
	if len(result.DeadFunctions) > 0 {
		var rows [][]string
		for _, fn := range result.DeadFunctions {
			confStr := fmt.Sprintf("%.0f%%", fn.Confidence*100)
			if fn.Confidence >= 0.9 {
				confStr = color.RedString(confStr)
			} else if fn.Confidence >= 0.8 {
				confStr = color.YellowString(confStr)
			}

			rows = append(rows, []string{
				fmt.Sprintf("%s:%d", fn.File, fn.Line),
				fn.Name,
				fn.Visibility,
				confStr,
			})
		}

		table := output.NewTable(
			"Potentially Dead Functions",
			[]string{"Location", "Function", "Visibility", "Confidence"},
			rows,
			nil,
			nil,
		)
		if err := formatter.Output(table); err != nil {
			return err
		}
	}

	// Variables table
	if len(result.DeadVariables) > 0 {
		var rows [][]string
		for _, v := range result.DeadVariables {
			rows = append(rows, []string{
				fmt.Sprintf("%s:%d", v.File, v.Line),
				v.Name,
				fmt.Sprintf("%.0f%%", v.Confidence*100),
			})
		}

		table := output.NewTable(
			"Potentially Dead Variables",
			[]string{"Location", "Variable", "Confidence"},
			rows,
			nil,
			nil,
		)
		if err := formatter.Output(table); err != nil {
			return err
		}
	}

	// Summary
	fmt.Printf("\nSummary: %d dead functions, %d dead variables across %d files (%.1f%% dead code)\n",
		result.Summary.TotalDeadFunctions,
		result.Summary.TotalDeadVariables,
		result.Summary.TotalFilesAnalyzed,
		result.Summary.DeadCodePercentage)

	return nil
}

func churnCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "days",
			Value: 30,
			Usage: "Number of days of history to analyze",
		},
		&cli.IntFlag{
			Name:  "top",
			Value: 20,
			Usage: "Show top N files by churn",
		},
	)
	return &cli.Command{
		Name:      "churn",
		Usage:     "Analyze git commit history for file churn",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runChurnCmd,
	}
}

func runChurnCmd(c *cli.Context) error {
	paths := getPaths(c)
	days := c.Int("days")
	topN := c.Int("top")

	if err := validateDays(days); err != nil {
		return err
	}

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing git history...")
	svc := analysis.New()
	result, err := svc.AnalyzeChurn(context.Background(), absPath, analysis.ChurnOptions{
		Days:    days,
		Top:     topN,
		Spinner: spinner,
	})
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("churn analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	files := result.Files
	if len(files) > topN {
		files = files[:topN]
	}

	var rows [][]string
	for _, fm := range files {
		scoreStr := fmt.Sprintf("%.2f", fm.ChurnScore)
		if fm.ChurnScore >= 0.8 {
			scoreStr = color.RedString(scoreStr)
		} else if fm.ChurnScore >= 0.5 {
			scoreStr = color.YellowString(scoreStr)
		}

		rows = append(rows, []string{
			fm.Path,
			fmt.Sprintf("%d", fm.Commits),
			fmt.Sprintf("%d", len(fm.UniqueAuthors)),
			fmt.Sprintf("+%d/-%d", fm.LinesAdded, fm.LinesDeleted),
			scoreStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("File Churn (Last %d Days)", days),
		[]string{"File", "Commits", "Authors", "Lines Changed", "Churn Score"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFilesChanged),
			fmt.Sprintf("Total Commits: %d", result.Summary.TotalCommits),
			fmt.Sprintf("Authors: %d", len(result.Summary.AuthorContributions)),
			"",
			fmt.Sprintf("Max: %.2f", result.Summary.MaxChurnScore),
		},
		result,
	)

	return formatter.Output(table)
}

func duplicatesCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "min-lines",
			Value: 6,
			Usage: "Minimum lines for clone detection",
		},
		&cli.Float64Flag{
			Name:  "threshold",
			Value: 0.8,
			Usage: "Similarity threshold (0.0-1.0)",
		},
	)
	return &cli.Command{
		Name:      "duplicates",
		Aliases:   []string{"dup", "clones"},
		Usage:     "Detect code clones and duplicates",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runDuplicatesCmd,
	}
}

func runDuplicatesCmd(c *cli.Context) error {
	paths := getPaths(c)
	minLines := c.Int("min-lines")
	threshold := c.Float64("threshold")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting duplicates...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeDuplicates(scanResult.Files, analysis.DuplicatesOptions{
		MinLines:            minLines,
		SimilarityThreshold: threshold,
		OnProgress:          tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		report := result.ToReport()
		return formatter.Output(report)
	}

	if len(result.Clones) == 0 {
		if formatter.Format() == output.FormatText {
			color.Green("No code duplicates found above %.0f%% similarity threshold", threshold*100)
		}
		return formatter.Output(result)
	}

	var rows [][]string
	for _, clone := range result.Clones {
		simStr := fmt.Sprintf("%.0f%%", clone.Similarity*100)
		if clone.Similarity >= 0.95 {
			simStr = color.RedString(simStr)
		} else if clone.Similarity >= 0.85 {
			simStr = color.YellowString(simStr)
		}

		rows = append(rows, []string{
			fmt.Sprintf("%s:%d-%d", clone.FileA, clone.StartLineA, clone.EndLineA),
			fmt.Sprintf("%s:%d-%d", clone.FileB, clone.StartLineB, clone.EndLineB),
			string(clone.Type),
			simStr,
			fmt.Sprintf("%d", clone.LinesA),
		})
	}

	table := output.NewTable(
		"Code Clones Detected",
		[]string{"Location A", "Location B", "Type", "Similarity", "Lines"},
		rows,
		[]string{
			fmt.Sprintf("Total Clones: %d", result.Summary.TotalClones),
			fmt.Sprintf("Type-1: %d", result.Summary.Type1Count),
			fmt.Sprintf("Type-2: %d", result.Summary.Type2Count),
			fmt.Sprintf("Type-3: %d", result.Summary.Type3Count),
			fmt.Sprintf("Avg Sim: %.0f%%", result.Summary.AvgSimilarity*100),
		},
		result,
	)

	return formatter.Output(table)
}

func defectCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.BoolFlag{
			Name:  "high-risk-only",
			Usage: "Show only high-risk files",
		},
	)
	return &cli.Command{
		Name:      "defect",
		Aliases:   []string{"predict"},
		Usage:     "Predict defect probability using PMAT weights",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runDefectCmd,
	}
}

func runDefectCmd(c *cli.Context) error {
	paths := getPaths(c)
	highRiskOnly := c.Bool("high-risk-only")

	// Use first path as repo root for git analysis
	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	svc := analysis.New()
	result, err := svc.AnalyzeDefects(context.Background(), repoPath, scanResult.Files, analysis.DefectOptions{
		HighRiskOnly: highRiskOnly,
	})
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Sort by probability (highest first)
	sort.Slice(result.Files, func(i, j int) bool {
		return result.Files[i].Probability > result.Files[j].Probability
	})

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		report := result.ToReport()
		return formatter.Output(report)
	}

	var rows [][]string
	for _, ds := range result.Files {
		if highRiskOnly && ds.RiskLevel != defect.RiskHigh {
			continue
		}

		probStr := fmt.Sprintf("%.0f%%", ds.Probability*100)
		riskStr := string(ds.RiskLevel)
		switch ds.RiskLevel {
		case defect.RiskHigh:
			probStr = color.RedString(probStr)
			riskStr = color.RedString(riskStr)
		case defect.RiskMedium:
			probStr = color.YellowString(probStr)
			riskStr = color.YellowString(riskStr)
		case defect.RiskLow:
			probStr = color.GreenString(probStr)
			riskStr = color.GreenString(riskStr)
		}

		rows = append(rows, []string{
			ds.FilePath,
			probStr,
			riskStr,
		})
	}

	table := output.NewTable(
		"Defect Probability Prediction",
		[]string{"File", "Probability", "Risk Level"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("High Risk: %d", result.Summary.HighRiskCount),
			fmt.Sprintf("Medium Risk: %d", result.Summary.MediumRiskCount),
			fmt.Sprintf("Avg Prob: %.0f%%", result.Summary.AvgProbability*100),
		},
		result,
	)

	return formatter.Output(table)
}

func changesCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "days",
			Value: 30,
			Usage: "Number of days of history to analyze",
		},
		&cli.IntFlag{
			Name:  "top",
			Value: 20,
			Usage: "Show top N commits by risk",
		},
		&cli.BoolFlag{
			Name:  "high-risk-only",
			Usage: "Show only high-risk commits",
		},
	)
	return &cli.Command{
		Name:      "changes",
		Aliases:   []string{"jit"},
		Usage:     "Analyze recent changes for defect risk (Kamei et al. 2013)",
		ArgsUsage: "[path]",
		Flags:     flags,
		Action:    runChangesCmd,
	}
}

func runChangesCmd(c *cli.Context) error {
	paths := getPaths(c)
	days := c.Int("days")
	topN := c.Int("top")
	highRiskOnly := c.Bool("high-risk-only")

	if err := validateDays(days); err != nil {
		return err
	}

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing recent changes...")
	svc := analysis.New()
	result, err := svc.AnalyzeChanges(context.Background(), absPath, analysis.ChangesOptions{
		Days: days,
	})
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("changes analysis failed (is this a git repository?): %w", err)
	}

	if result.Summary.TotalCommits == 0 {
		color.Yellow("No commits found in the last %d days", days)
		return nil
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	commits := result.Commits
	if len(commits) > topN {
		commits = commits[:topN]
	}

	var rows [][]string
	for _, cr := range commits {
		if highRiskOnly && cr.RiskLevel != changes.RiskLevelHigh {
			continue
		}

		scoreStr := fmt.Sprintf("%.2f", cr.RiskScore)
		riskStr := string(cr.RiskLevel)
		switch cr.RiskLevel {
		case changes.RiskLevelHigh:
			scoreStr = color.RedString(scoreStr)
			riskStr = color.RedString(riskStr)
		case changes.RiskLevelMedium:
			scoreStr = color.YellowString(scoreStr)
			riskStr = color.YellowString(riskStr)
		case changes.RiskLevelLow:
			scoreStr = color.GreenString(scoreStr)
			riskStr = color.GreenString(riskStr)
		}

		rows = append(rows, []string{
			cr.CommitHash[:8],
			cr.Author,
			cr.Message,
			scoreStr,
			riskStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Change Risk Analysis (Last %d Days)", days),
		[]string{"Commit", "Author", "Message", "Risk Score", "Level"},
		rows,
		[]string{
			fmt.Sprintf("Total Commits: %d", result.Summary.TotalCommits),
			fmt.Sprintf("High Risk: %d", result.Summary.HighRiskCount),
			fmt.Sprintf("Medium Risk: %d", result.Summary.MediumRiskCount),
			fmt.Sprintf("Bug Fixes: %d", result.Summary.BugFixCount),
			fmt.Sprintf("Avg Score: %.2f", result.Summary.AvgRiskScore),
		},
		result,
	)

	return formatter.Output(table)
}

func tdgCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "hotspots",
			Value: 10,
			Usage: "Number of hotspots to show",
		},
		&cli.BoolFlag{
			Name:  "penalties",
			Usage: "Show applied penalties",
		},
	)
	return &cli.Command{
		Name:      "tdg",
		Usage:     "Calculate Technical Debt Gradient scores (0-100, higher is better)",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runTDGCmd,
	}
}

func runTDGCmd(c *cli.Context) error {
	paths := getPaths(c)
	hotspots := c.Int("hotspots")
	showPenalties := c.Bool("penalties")

	svc := analysis.New()

	// Handle directory vs single file
	var project *tdg.ProjectScore
	var err error

	if len(paths) == 1 {
		info, statErr := os.Stat(paths[0])
		if statErr != nil {
			return fmt.Errorf("invalid path %s: %w", paths[0], statErr)
		}

		if info.IsDir() {
			project, err = svc.AnalyzeTDG(paths[0])
		} else {
			tdgAnalyzer := tdg.New()
			defer tdgAnalyzer.Close()
			score, fileErr := tdgAnalyzer.AnalyzeFile(paths[0])
			if fileErr != nil {
				return fmt.Errorf("analysis failed: %w", fileErr)
			}
			projectScore := tdg.AggregateProjectScore([]tdg.Score{score})
			project = &projectScore
		}
	} else {
		var allScores []tdg.Score
		for _, path := range paths {
			info, statErr := os.Stat(path)
			if statErr != nil {
				return fmt.Errorf("invalid path %s: %w", path, statErr)
			}

			if info.IsDir() {
				dirProject, dirErr := svc.AnalyzeTDG(path)
				if dirErr != nil {
					return fmt.Errorf("analysis failed for %s: %w", path, dirErr)
				}
				allScores = append(allScores, dirProject.Files...)
			} else {
				tdgAnalyzer := tdg.New()
				score, fileErr := tdgAnalyzer.AnalyzeFile(path)
				tdgAnalyzer.Close()
				if fileErr != nil {
					return fmt.Errorf("analysis failed for %s: %w", path, fileErr)
				}
				allScores = append(allScores, score)
			}
		}
		projectScore := tdg.AggregateProjectScore(allScores)
		project = &projectScore
	}

	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, fmtErr := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if fmtErr != nil {
		return fmtErr
	}
	defer formatter.Close()

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		report := project.ToReport(hotspots)
		return formatter.Output(report)
	}

	// Sort by score (lowest first - worst quality)
	files := project.Files
	sort.Slice(files, func(i, j int) bool {
		return files[i].Total < files[j].Total
	})

	// Show top hotspots (worst scores)
	filesToShow := files
	if len(filesToShow) > hotspots {
		filesToShow = filesToShow[:hotspots]
	}

	var rows [][]string
	for _, ts := range filesToShow {
		scoreStr := fmt.Sprintf("%.1f", ts.Total)
		gradeStr := string(ts.Grade)

		// Color by grade
		switch {
		case ts.Total >= 90:
			scoreStr = color.GreenString(scoreStr)
			gradeStr = color.GreenString(gradeStr)
		case ts.Total >= 70:
			scoreStr = color.YellowString(scoreStr)
			gradeStr = color.YellowString(gradeStr)
		default:
			scoreStr = color.RedString(scoreStr)
			gradeStr = color.RedString(gradeStr)
		}

		row := []string{
			ts.FilePath,
			scoreStr,
			gradeStr,
			fmt.Sprintf("%.1f", ts.StructuralComplexity),
			fmt.Sprintf("%.1f", ts.SemanticComplexity),
			fmt.Sprintf("%.1f", ts.CouplingScore),
		}

		if showPenalties && len(ts.PenaltiesApplied) > 0 {
			var penaltyStrs []string
			for _, p := range ts.PenaltiesApplied {
				penaltyStrs = append(penaltyStrs, fmt.Sprintf("%.1f: %s", p.Amount, p.Issue))
			}
			row = append(row, strings.Join(penaltyStrs, "; "))
		}

		rows = append(rows, row)
	}

	headers := []string{"File", "Score", "Grade", "Struct", "Semantic", "Coupling"}
	if showPenalties {
		headers = append(headers, "Penalties")
	}

	table := output.NewTable(
		fmt.Sprintf("TDG Analysis (Top %d Hotspots)", hotspots),
		headers,
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", project.TotalFiles),
			fmt.Sprintf("Average Score: %.1f", project.AverageScore),
			fmt.Sprintf("Average Grade: %s", project.AverageGrade),
		},
		project,
	)

	if err := formatter.Output(table); err != nil {
		return err
	}

	// Display grade distribution
	if formatter.Format() == output.FormatText && len(project.GradeDistribution) > 0 {
		fmt.Fprintln(formatter.Writer())
		if formatter.Colored() {
			color.Cyan("Grade Distribution:")
		} else {
			fmt.Fprintln(formatter.Writer(), "Grade Distribution:")
		}

		// Define order of grades
		gradeOrder := []tdg.Grade{
			tdg.GradeAPlus,
			tdg.GradeA,
			tdg.GradeAMinus,
			tdg.GradeBPlus,
			tdg.GradeB,
			tdg.GradeBMinus,
			tdg.GradeCPlus,
			tdg.GradeC,
			tdg.GradeCMinus,
			tdg.GradeD,
			tdg.GradeF,
		}

		for _, grade := range gradeOrder {
			if count := project.GradeDistribution[grade]; count > 0 {
				percentage := float64(count) / float64(project.TotalFiles) * 100
				fmt.Fprintf(formatter.Writer(), "- %s: %d files (%.1f%%)\n", grade, count, percentage)
			}
		}
	}

	return nil
}

func graphCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.StringFlag{
			Name:  "scope",
			Value: "module",
			Usage: "Scope: file, function, module, package",
		},
		&cli.BoolFlag{
			Name:  "metrics",
			Usage: "Include PageRank and centrality metrics",
		},
	)
	return &cli.Command{
		Name:      "graph",
		Aliases:   []string{"dag"},
		Usage:     "Generate dependency graph (Mermaid output)",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runGraphCmd,
	}
}

func runGraphCmd(c *cli.Context) error {
	paths := getPaths(c)
	scope := c.String("scope")
	includeMetrics := c.Bool("metrics")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Building dependency graph...", len(scanResult.Files))
	svc := analysis.New()
	graphResult, metrics, err := svc.AnalyzeGraph(scanResult.Files, analysis.GraphOptions{
		Scope:          graph.Scope(scope),
		IncludeMetrics: includeMetrics,
		OnProgress:     tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// For JSON/TOON, output structured data
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		if includeMetrics && metrics != nil {
			return formatter.Output(struct {
				Graph   *graph.DependencyGraph `json:"graph" toon:"graph"`
				Metrics *graph.Metrics         `json:"metrics" toon:"metrics"`
			}{graphResult, metrics})
		}
		return formatter.Output(graphResult)
	}

	// Generate Mermaid diagram for text/markdown
	fmt.Fprintln(formatter.Writer(), "```mermaid")
	fmt.Fprintln(formatter.Writer(), "graph TD")
	for _, node := range graphResult.Nodes {
		fmt.Fprintf(formatter.Writer(), "    %s[%s]\n", sanitizeID(node.ID), node.Name)
	}
	for _, edge := range graphResult.Edges {
		fmt.Fprintf(formatter.Writer(), "    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
	}
	fmt.Fprintln(formatter.Writer(), "```")

	if includeMetrics && metrics != nil {
		fmt.Fprintln(formatter.Writer())
		if formatter.Colored() {
			color.Cyan("Graph Metrics:")
		} else {
			fmt.Fprintln(formatter.Writer(), "Graph Metrics:")
		}
		fmt.Fprintf(formatter.Writer(), "  Nodes: %d\n", metrics.Summary.TotalNodes)
		fmt.Fprintf(formatter.Writer(), "  Edges: %d\n", metrics.Summary.TotalEdges)
		fmt.Fprintf(formatter.Writer(), "  Avg Degree: %.2f\n", metrics.Summary.AvgDegree)
		fmt.Fprintf(formatter.Writer(), "  Density: %.4f\n", metrics.Summary.Density)

		if len(metrics.NodeMetrics) > 0 {
			fmt.Fprintln(formatter.Writer())
			if formatter.Colored() {
				color.Cyan("Top Nodes by PageRank:")
			} else {
				fmt.Fprintln(formatter.Writer(), "Top Nodes by PageRank:")
			}
			sort.Slice(metrics.NodeMetrics, func(i, j int) bool {
				return metrics.NodeMetrics[i].PageRank > metrics.NodeMetrics[j].PageRank
			})
			for i, nm := range metrics.NodeMetrics {
				if i >= 5 {
					break
				}
				fmt.Fprintf(formatter.Writer(), "  %s: %.4f (in: %d, out: %d)\n",
					nm.Name, nm.PageRank, nm.InDegree, nm.OutDegree)
			}
		}
	}

	return nil
}

func sanitizeID(id string) string {
	// Replace problematic characters for Mermaid
	result := ""
	for _, c := range id {
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_' {
			result += string(c)
		} else {
			result += "_"
		}
	}
	return result
}

func lintHotspotCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "top",
			Value: 10,
			Usage: "Show top N files",
		},
	)
	return &cli.Command{
		Name:      "lint-hotspot",
		Aliases:   []string{"lh"},
		Usage:     "Identify files with high lint violation density",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runLintHotspotCmd,
	}
}

func runLintHotspotCmd(c *cli.Context) error {
	// Lint hotspot uses complexity as a proxy for now
	// (full implementation would integrate with linters)
	paths := getPaths(c)
	topN := c.Int("top")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	// Use complexity as hotspot indicator
	tracker := progress.NewTracker("Analyzing hotspots...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeComplexity(scanResult.Files, analysis.ComplexityOptions{
		OnProgress: tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Sort by total complexity (as proxy for lint density)
	sort.Slice(result.Files, func(i, j int) bool {
		return result.Files[i].TotalCyclomatic+result.Files[i].TotalCognitive >
			result.Files[j].TotalCyclomatic+result.Files[j].TotalCognitive
	})

	filesToShow := result.Files
	if len(filesToShow) > topN {
		filesToShow = filesToShow[:topN]
	}

	var rows [][]string
	for _, fc := range filesToShow {
		score := fc.TotalCyclomatic + fc.TotalCognitive
		scoreStr := fmt.Sprintf("%d", score)
		if score > 100 {
			scoreStr = color.RedString(scoreStr)
		} else if score > 50 {
			scoreStr = color.YellowString(scoreStr)
		}

		rows = append(rows, []string{
			fc.Path,
			fmt.Sprintf("%d", len(fc.Functions)),
			fmt.Sprintf("%d", fc.TotalCyclomatic),
			fmt.Sprintf("%d", fc.TotalCognitive),
			scoreStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Complexity Hotspots (Top %d)", topN),
		[]string{"File", "Functions", "Cyclomatic", "Cognitive", "Total Score"},
		rows,
		nil,
		result,
	)

	return formatter.Output(table)
}

func hotspotCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "top",
			Value: 20,
			Usage: "Show top N files by hotspot score",
		},
		&cli.IntFlag{
			Name:  "days",
			Value: 30,
			Usage: "Number of days of git history to analyze",
		},
	)
	return &cli.Command{
		Name:      "hotspot",
		Aliases:   []string{"hs"},
		Usage:     "Identify code hotspots (high churn + high complexity)",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runHotspotCmd,
	}
}

func runHotspotCmd(c *cli.Context) error {
	paths := getPaths(c)
	topN := c.Int("top")
	days := c.Int("days")

	if err := validateDays(days); err != nil {
		return err
	}

	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Analyzing hotspots...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeHotspots(context.Background(), repoPath, scanResult.Files, analysis.HotspotOptions{
		Days:       days,
		Top:        topN,
		OnProgress: tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("hotspot analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	filesToShow := result.Files
	if len(filesToShow) > topN {
		filesToShow = filesToShow[:topN]
	}

	var rows [][]string
	for _, fh := range filesToShow {
		hotspotStr := fmt.Sprintf("%.2f", fh.HotspotScore)
		if fh.HotspotScore >= 0.7 {
			hotspotStr = color.RedString(hotspotStr)
		} else if fh.HotspotScore >= 0.4 {
			hotspotStr = color.YellowString(hotspotStr)
		}

		rows = append(rows, []string{
			fh.Path,
			hotspotStr,
			fmt.Sprintf("%.2f", fh.ChurnScore),
			fmt.Sprintf("%.2f", fh.ComplexityScore),
			fmt.Sprintf("%d", fh.Commits),
			fmt.Sprintf("%.1f", fh.AvgCognitive),
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Code Hotspots (Top %d, Last %d Days)", topN, days),
		[]string{"File", "Hotspot", "Churn", "Complexity", "Commits", "Avg Cognitive"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("Hotspots (>0.5): %d", result.Summary.HotspotCount),
			fmt.Sprintf("Max Score: %.2f", result.Summary.MaxHotspotScore),
			fmt.Sprintf("Avg Score: %.2f", result.Summary.AvgHotspotScore),
		},
		result,
	)

	return formatter.Output(table)
}

func smellsCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "hub-threshold",
			Value: 20,
			Usage: "Fan-in + fan-out threshold for hub detection",
		},
		&cli.IntFlag{
			Name:  "god-fan-in",
			Value: 10,
			Usage: "Minimum fan-in for god component detection",
		},
		&cli.IntFlag{
			Name:  "god-fan-out",
			Value: 10,
			Usage: "Minimum fan-out for god component detection",
		},
		&cli.Float64Flag{
			Name:  "instability-diff",
			Value: 0.4,
			Usage: "Max instability difference for unstable dependency detection",
		},
	)
	return &cli.Command{
		Name:      "smells",
		Usage:     "Detect architectural smells (cycles, hubs, god components, unstable dependencies)",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runSmellsCmd,
	}
}

func runSmellsCmd(c *cli.Context) error {
	paths := getPaths(c)
	hubThreshold := c.Int("hub-threshold")
	godFanIn := c.Int("god-fan-in")
	godFanOut := c.Int("god-fan-out")
	instabilityDiff := c.Float64("instability-diff")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting architectural smells...", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeSmells(scanResult.Files, analysis.SmellOptions{
		HubThreshold:          hubThreshold,
		GodFanInThreshold:     godFanIn,
		GodFanOutThreshold:    godFanOut,
		InstabilityDifference: instabilityDiff,
		OnProgress:            tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("smell analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	if len(result.Smells) == 0 {
		color.Green("No architectural smells detected")
		return nil
	}

	var rows [][]string
	for _, smell := range result.Smells {
		severityStr := string(smell.Severity)
		switch smell.Severity {
		case smells.SeverityCritical:
			severityStr = color.RedString(severityStr)
		case smells.SeverityHigh:
			severityStr = color.YellowString(severityStr)
		}

		componentsStr := smell.Components[0]
		if len(smell.Components) > 1 {
			componentsStr = fmt.Sprintf("%s (+%d more)", smell.Components[0], len(smell.Components)-1)
		}

		rows = append(rows, []string{
			string(smell.Type),
			severityStr,
			componentsStr,
			smell.Description,
		})
	}

	table := output.NewTable(
		"Architectural Smells",
		[]string{"Type", "Severity", "Components", "Description"},
		rows,
		[]string{
			fmt.Sprintf("Total Smells: %d", result.Summary.TotalSmells),
			fmt.Sprintf("Critical: %d", result.Summary.CriticalCount),
			fmt.Sprintf("High: %d", result.Summary.HighCount),
			fmt.Sprintf("Cycles: %d", result.Summary.CyclicCount),
			fmt.Sprintf("Hubs: %d", result.Summary.HubCount),
			fmt.Sprintf("Gods: %d", result.Summary.GodCount),
		},
		result,
	)

	return formatter.Output(table)
}

func temporalCouplingCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "top",
			Value: 20,
			Usage: "Show top N file pairs by coupling strength",
		},
		&cli.IntFlag{
			Name:  "days",
			Value: 30,
			Usage: "Number of days of git history to analyze",
		},
		&cli.IntFlag{
			Name:  "min-cochanges",
			Value: 3,
			Usage: "Minimum number of co-changes to consider files coupled",
		},
	)
	return &cli.Command{
		Name:      "temporal-coupling",
		Aliases:   []string{"tc"},
		Usage:     "Identify files that frequently change together",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runTemporalCouplingCmd,
	}
}

func runTemporalCouplingCmd(c *cli.Context) error {
	paths := getPaths(c)
	topN := c.Int("top")
	days := c.Int("days")
	minCochanges := c.Int("min-cochanges")

	if err := validateDays(days); err != nil {
		return err
	}

	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	spinner := progress.NewSpinner("Analyzing temporal coupling...")
	svc := analysis.New()
	result, err := svc.AnalyzeTemporalCoupling(context.Background(), repoPath, analysis.TemporalCouplingOptions{
		Days:         days,
		MinCochanges: minCochanges,
		Top:          topN,
	})
	spinner.FinishSuccess()
	if err != nil {
		return fmt.Errorf("temporal coupling analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	couplingsToShow := result.Couplings
	if len(couplingsToShow) > topN {
		couplingsToShow = couplingsToShow[:topN]
	}

	var rows [][]string
	for _, fc := range couplingsToShow {
		strengthStr := fmt.Sprintf("%.2f", fc.CouplingStrength)
		if fc.CouplingStrength >= 0.7 {
			strengthStr = color.RedString(strengthStr)
		} else if fc.CouplingStrength >= 0.4 {
			strengthStr = color.YellowString(strengthStr)
		}

		rows = append(rows, []string{
			fc.FileA,
			fc.FileB,
			fmt.Sprintf("%d", fc.CochangeCount),
			strengthStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Temporal Coupling (Top %d, Last %d Days, Min %d Co-changes)", topN, days, minCochanges),
		[]string{"File A", "File B", "Co-changes", "Strength"},
		rows,
		[]string{
			fmt.Sprintf("Total Couplings: %d", result.Summary.TotalCouplings),
			fmt.Sprintf("Strong (>0.5): %d", result.Summary.StrongCouplings),
			fmt.Sprintf("Files Analyzed: %d", result.Summary.TotalFilesAnalyzed),
			fmt.Sprintf("Max Strength: %.2f", result.Summary.MaxCouplingStrength),
		},
		result,
	)

	return formatter.Output(table)
}

func ownershipCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "top",
			Value: 20,
			Usage: "Show top N files by ownership concentration",
		},
		&cli.BoolFlag{
			Name:  "include-trivial",
			Usage: "Include trivial lines (imports, braces, blanks) in ownership calculation",
		},
	)
	return &cli.Command{
		Name:      "ownership",
		Aliases:   []string{"own", "bus-factor"},
		Usage:     "Analyze code ownership and bus factor risk",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runOwnershipCmd,
	}
}

func runOwnershipCmd(c *cli.Context) error {
	paths := getPaths(c)
	topN := c.Int("top")
	includeTrivial := c.Bool("include-trivial")

	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Analyzing ownership", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeOwnership(repoPath, scanResult.Files, analysis.OwnershipOptions{
		Top:            topN,
		IncludeTrivial: includeTrivial,
		OnProgress:     tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("ownership analysis failed (is this a git repository?): %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	filesToShow := result.Files
	if len(filesToShow) > topN {
		filesToShow = filesToShow[:topN]
	}

	var rows [][]string
	for _, fo := range filesToShow {
		concStr := fmt.Sprintf("%.2f", fo.Concentration)
		if fo.Concentration >= 0.9 {
			concStr = color.RedString(concStr)
		} else if fo.Concentration >= 0.7 {
			concStr = color.YellowString(concStr)
		}

		siloStr := ""
		if fo.IsSilo {
			siloStr = color.RedString("SILO")
		}

		rows = append(rows, []string{
			fo.Path,
			fo.PrimaryOwner,
			fmt.Sprintf("%.0f%%", fo.OwnershipPercent),
			concStr,
			fmt.Sprintf("%d", len(fo.Contributors)),
			siloStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Code Ownership (Top %d by Concentration)", topN),
		[]string{"Path", "Primary Owner", "Ownership", "Concentration", "Contributors", "Silo"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", result.Summary.TotalFiles),
			fmt.Sprintf("Bus Factor: %d", result.Summary.BusFactor),
			fmt.Sprintf("Silos: %d", result.Summary.SiloCount),
			fmt.Sprintf("Avg Contributors: %.1f", result.Summary.AvgContributors),
		},
		result,
	)

	return formatter.Output(table)
}

func cohesionCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "top",
			Value: 20,
			Usage: "Show top N classes by LCOM (least cohesive first)",
		},
		&cli.BoolFlag{
			Name:  "include-tests",
			Usage: "Include test files in analysis",
		},
		&cli.StringFlag{
			Name:  "sort",
			Value: "lcom",
			Usage: "Sort by: lcom, wmc, cbo, dit",
		},
	)
	return &cli.Command{
		Name:      "cohesion",
		Aliases:   []string{"ck"},
		Usage:     "Analyze CK (Chidamber-Kemerer) OO metrics",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runCohesionCmd,
	}
}

func runCohesionCmd(c *cli.Context) error {
	paths := getPaths(c)
	topN := c.Int("top")
	includeTests := c.Bool("include-tests")
	sortBy := getSort(c, "lcom")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Analyzing CK metrics", len(scanResult.Files))
	svc := analysis.New()
	result, err := svc.AnalyzeCohesion(scanResult.Files, analysis.CohesionOptions{
		IncludeTests: includeTests,
		Sort:         sortBy,
		Top:          topN,
		OnProgress:   tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("cohesion analysis failed: %w", err)
	}

	if len(result.Classes) == 0 {
		color.Yellow("No OO classes found (CK metrics only apply to Java, Python, TypeScript, etc.)")
		return nil
	}

	// Sort by requested metric
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

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	classesToShow := result.Classes
	if len(classesToShow) > topN {
		classesToShow = classesToShow[:topN]
	}

	var rows [][]string
	for _, cls := range classesToShow {
		lcomStr := fmt.Sprintf("%d", cls.LCOM)
		if cls.LCOM > 1 {
			lcomStr = color.YellowString(lcomStr)
		}
		if cls.LCOM > 3 {
			lcomStr = color.RedString(fmt.Sprintf("%d", cls.LCOM))
		}

		wmcStr := fmt.Sprintf("%d", cls.WMC)
		if cls.WMC > 30 {
			wmcStr = color.RedString(wmcStr)
		} else if cls.WMC > 15 {
			wmcStr = color.YellowString(wmcStr)
		}

		ditStr := fmt.Sprintf("%d", cls.DIT)
		if cls.DIT >= 5 {
			ditStr = color.RedString(ditStr)
		} else if cls.DIT >= 4 {
			ditStr = color.YellowString(ditStr)
		}

		nocStr := fmt.Sprintf("%d", cls.NOC)
		if cls.NOC >= 6 {
			nocStr = color.RedString(nocStr)
		} else if cls.NOC >= 4 {
			nocStr = color.YellowString(nocStr)
		}

		rows = append(rows, []string{
			cls.ClassName,
			cls.Path,
			wmcStr,
			fmt.Sprintf("%d", cls.CBO),
			fmt.Sprintf("%d", cls.RFC),
			lcomStr,
			ditStr,
			nocStr,
			fmt.Sprintf("%d", cls.NOM),
		})
	}

	table := output.NewTable(
		fmt.Sprintf("CK Metrics (Top %d by %s)", topN, sortBy),
		[]string{"Class", "Path", "WMC", "CBO", "RFC", "LCOM", "DIT", "NOC", "Methods"},
		rows,
		[]string{
			fmt.Sprintf("Total Classes: %d", result.Summary.TotalClasses),
			fmt.Sprintf("Low Cohesion (LCOM>1): %d", result.Summary.LowCohesionCount),
			fmt.Sprintf("Avg WMC: %.1f", result.Summary.AvgWMC),
			fmt.Sprintf("Max WMC: %d", result.Summary.MaxWMC),
			fmt.Sprintf("Max DIT: %d", result.Summary.MaxDIT),
		},
		result,
	)

	return formatter.Output(table)
}

func flagsCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.StringSliceFlag{
			Name:  "provider",
			Usage: "Filter by provider (launchdarkly, split, unleash, posthog, flipper)",
		},
		&cli.BoolFlag{
			Name:  "no-git",
			Usage: "Skip git history analysis for staleness",
		},
		&cli.StringFlag{
			Name:  "min-priority",
			Value: "",
			Usage: "Filter by minimum priority (LOW, MEDIUM, HIGH, CRITICAL)",
		},
		&cli.IntFlag{
			Name:  "top",
			Value: 0,
			Usage: "Show only top N flags by priority (0 = all)",
		},
	)
	return &cli.Command{
		Name:      "flags",
		Aliases:   []string{"ff"},
		Usage:     "Detect and analyze feature flags",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runFlagsCmd,
	}
}

func runFlagsCmd(c *cli.Context) error {
	paths := getPaths(c)
	providers := c.StringSlice("provider")
	noGit := c.Bool("no-git")
	minPriority := c.String("min-priority")
	topN := c.Int("top")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	tracker := progress.NewTracker("Detecting feature flags...", len(scanResult.Files))

	// Load config from flag or default locations
	var svc *analysis.Service
	if configPath := c.String("config"); configPath != "" {
		cfg, err := config.Load(configPath)
		if err != nil {
			return fmt.Errorf("failed to load config: %w", err)
		}
		svc = analysis.New(analysis.WithConfig(cfg))
	} else {
		svc = analysis.New()
	}

	result, err := svc.AnalyzeFeatureFlags(scanResult.Files, analysis.FeatureFlagOptions{
		Providers:  providers,
		IncludeGit: !noGit,
		OnProgress: tracker.Tick,
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("feature flag analysis failed: %w", err)
	}

	if len(result.Flags) == 0 {
		color.Yellow("No feature flags detected")
		return nil
	}

	// Filter by minimum priority if specified
	flags := result.Flags
	if minPriority != "" {
		minPriority = strings.ToUpper(minPriority)
		priorityOrder := map[string]int{
			"LOW": 0, "MEDIUM": 1, "HIGH": 2, "CRITICAL": 3,
		}
		minOrder, ok := priorityOrder[minPriority]
		if ok {
			filtered := make([]featureflags.FlagAnalysis, 0)
			for _, f := range flags {
				if priorityOrder[f.Priority.Level] >= minOrder {
					filtered = append(filtered, f)
				}
			}
			flags = filtered
		}
	}

	// Limit results if requested
	if topN > 0 && len(flags) > topN {
		flags = flags[:topN]
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	var rows [][]string
	for _, f := range flags {
		priorityStr := f.Priority.Level
		switch f.Priority.Level {
		case "CRITICAL":
			priorityStr = color.RedString(priorityStr)
		case "HIGH":
			priorityStr = color.YellowString(priorityStr)
		case "MEDIUM":
			priorityStr = color.CyanString(priorityStr)
		}

		staleness := "-"
		if f.Staleness != nil {
			staleness = fmt.Sprintf("%dd", f.Staleness.DaysSinceIntro)
		}

		rows = append(rows, []string{
			f.FlagKey,
			f.Provider,
			fmt.Sprintf("%d", len(f.References)),
			fmt.Sprintf("%d", f.Complexity.FileSpread),
			fmt.Sprintf("%d", f.Complexity.MaxNestingDepth),
			staleness,
			priorityStr,
		})
	}

	table := output.NewTable(
		"Feature Flag Analysis",
		[]string{"Flag Key", "Provider", "Refs", "Files", "Nesting", "Age", "Priority"},
		rows,
		[]string{
			fmt.Sprintf("Total Flags: %d", result.Summary.TotalFlags),
			fmt.Sprintf("Total References: %d", result.Summary.TotalReferences),
			fmt.Sprintf("Critical: %d, High: %d, Medium: %d, Low: %d",
				result.Summary.ByPriority["CRITICAL"],
				result.Summary.ByPriority["HIGH"],
				result.Summary.ByPriority["MEDIUM"],
				result.Summary.ByPriority["LOW"]),
		},
		result,
	)

	return formatter.Output(table)
}

func contextCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.BoolFlag{
			Name:  "include-metrics",
			Usage: "Include complexity and quality metrics",
		},
		&cli.BoolFlag{
			Name:  "include-graph",
			Usage: "Include dependency graph",
		},
		&cli.BoolFlag{
			Name:  "repo-map",
			Usage: "Generate PageRank-ranked symbol map",
		},
		&cli.IntFlag{
			Name:  "top",
			Value: 50,
			Usage: "Number of top symbols to include in repo map",
		},
	)
	return &cli.Command{
		Name:      "context",
		Aliases:   []string{"ctx"},
		Usage:     "Generate deep context for LLM consumption",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runContextCmd,
	}
}

func runContextCmd(c *cli.Context) error {
	paths := getPaths(c)
	includeMetrics := c.Bool("include-metrics")
	includeGraph := c.Bool("include-graph")
	repoMap := c.Bool("repo-map")
	topN := c.Int("top")

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	files := scanResult.Files
	langGroups := scanResult.LanguageGroups

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Output project structure
	fmt.Println("# Project Context")
	fmt.Println()
	fmt.Printf("## Overview\n")
	fmt.Printf("- **Paths**: %v\n", paths)
	fmt.Printf("- **Total Files**: %d\n", len(files))
	fmt.Println()

	fmt.Println("## Language Distribution")
	for lang, langFiles := range langGroups {
		fmt.Printf("- **%s**: %d files\n", lang, len(langFiles))
	}
	fmt.Println()

	fmt.Println("## File Structure")
	for _, f := range files {
		fmt.Printf("- %s\n", f)
	}

	if includeMetrics {
		fmt.Println()
		fmt.Println("## Complexity Metrics")
		svc := analysis.New()
		cxResult, cxErr := svc.AnalyzeComplexity(files, analysis.ComplexityOptions{})
		if cxErr == nil {
			fmt.Printf("- **Total Functions**: %d\n", cxResult.Summary.TotalFunctions)
			fmt.Printf("- **Median Cyclomatic (P50)**: %d\n", cxResult.Summary.P50Cyclomatic)
			fmt.Printf("- **Median Cognitive (P50)**: %d\n", cxResult.Summary.P50Cognitive)
			fmt.Printf("- **90th Percentile Cyclomatic**: %d\n", cxResult.Summary.P90Cyclomatic)
			fmt.Printf("- **90th Percentile Cognitive**: %d\n", cxResult.Summary.P90Cognitive)
			fmt.Printf("- **Max Cyclomatic**: %d\n", cxResult.Summary.MaxCyclomatic)
			fmt.Printf("- **Max Cognitive**: %d\n", cxResult.Summary.MaxCognitive)
		}
	}

	if includeGraph {
		fmt.Println()
		fmt.Println("## Dependency Graph")
		graphSvc := analysis.New()
		graphData, _, graphErr := graphSvc.AnalyzeGraph(files, analysis.GraphOptions{
			Scope: graph.ScopeFile,
		})
		if graphErr == nil {
			fmt.Println("```mermaid")
			fmt.Println("graph TD")
			for _, node := range graphData.Nodes {
				fmt.Printf("    %s[%s]\n", sanitizeID(node.ID), node.Name)
			}
			for _, edge := range graphData.Edges {
				fmt.Printf("    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
			}
			fmt.Println("```")
		}
	}

	if repoMap {
		fmt.Println()
		fmt.Println("## Repository Map")

		spinner := progress.NewSpinner("Generating repo map...")
		rmSvc := analysis.New()
		rm, rmErr := rmSvc.AnalyzeRepoMap(files, analysis.RepoMapOptions{Top: topN})
		spinner.FinishSuccess()

		if rmErr == nil {
			// Check if we should output using formatter
			formatStr := getFormat(c)
			if formatStr != "" && formatStr != "text" {
				// Use formatter for non-text output
				topSymbols := rm.TopN(topN)

				var rows [][]string
				for _, s := range topSymbols {
					rows = append(rows, []string{
						s.Name,
						s.Kind,
						s.File,
						fmt.Sprintf("%d", s.Line),
						fmt.Sprintf("%.4f", s.PageRank),
						fmt.Sprintf("%d", s.InDegree),
						fmt.Sprintf("%d", s.OutDegree),
					})
				}

				table := output.NewTable(
					fmt.Sprintf("Repository Map (Top %d Symbols by PageRank)", topN),
					[]string{"Name", "Kind", "File", "Line", "PageRank", "In-Degree", "Out-Degree"},
					rows,
					[]string{
						fmt.Sprintf("Total Symbols: %d", rm.Summary.TotalSymbols),
						fmt.Sprintf("Total Files: %d", rm.Summary.TotalFiles),
						fmt.Sprintf("Avg Connections: %.1f", rm.Summary.AvgConnections),
					},
					rm,
				)

				if err := formatter.Output(table); err != nil {
					return err
				}
			} else {
				// Text/markdown output
				topSymbols := rm.TopN(topN)
				fmt.Printf("Top %d symbols by PageRank:\n\n", len(topSymbols))
				fmt.Println("| Symbol | Kind | File | Line | PageRank |")
				fmt.Println("|--------|------|------|------|----------|")
				for _, s := range topSymbols {
					fmt.Printf("| %s | %s | %s | %d | %.4f |\n",
						s.Name, s.Kind, s.File, s.Line, s.PageRank)
				}
				fmt.Println()
				fmt.Printf("- **Total Symbols**: %d\n", rm.Summary.TotalSymbols)
				fmt.Printf("- **Total Files**: %d\n", rm.Summary.TotalFiles)
				fmt.Printf("- **Max PageRank**: %.4f\n", rm.Summary.MaxPageRank)
			}
		}
	}

	return nil
}

func analyzeCmd() *cli.Command {
	flags := append(outputFlags(), &cli.StringSliceFlag{
		Name:  "exclude",
		Usage: "Analyzers to exclude (when running all)",
	})
	return &cli.Command{
		Name:      "analyze",
		Aliases:   []string{"a"},
		Usage:     "Run code analysis (all analyzers if no subcommand specified)",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runAnalyzeCmd,
		Subcommands: []*cli.Command{
			complexityCmd(),
			satdCmd(),
			deadcodeCmd(),
			churnCmd(),
			duplicatesCmd(),
			defectCmd(),
			changesCmd(),
			tdgCmd(),
			graphCmd(),
			lintHotspotCmd(),
			hotspotCmd(),
			smellsCmd(),
			temporalCouplingCmd(),
			ownershipCmd(),
			cohesionCmd(),
			flagsCmd(),
		},
	}
}

func runAnalyzeCmd(c *cli.Context) error {
	paths := getPaths(c)
	exclude := c.StringSlice("exclude")

	// Use first path as repo root for git-based analyzers
	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	files := scanResult.Files

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	excludeSet := make(map[string]bool)
	for _, e := range exclude {
		excludeSet[e] = true
	}

	// Comprehensive analysis results
	type FullAnalysis struct {
		Complexity       *complexity.Analysis   `json:"complexity,omitempty"`
		SATD             *satd.Analysis         `json:"satd,omitempty"`
		DeadCode         *deadcode.Analysis     `json:"dead_code,omitempty"`
		Churn            *churn.Analysis        `json:"churn,omitempty"`
		Clones           *duplicates.Analysis   `json:"clones,omitempty"`
		Defect           *defect.Analysis       `json:"defect,omitempty"`
		TDG              *tdg.ProjectScore      `json:"tdg,omitempty"`
		Hotspots         *hotspot.Analysis      `json:"hotspots,omitempty"`
		Smells           *smells.Analysis       `json:"smells,omitempty"`
		Ownership        *ownership.Analysis    `json:"ownership,omitempty"`
		TemporalCoupling *temporal.Analysis     `json:"temporal_coupling,omitempty"`
		Cohesion         *cohesion.Analysis     `json:"cohesion,omitempty"`
		FeatureFlags     *featureflags.Analysis `json:"feature_flags,omitempty"`
	}
	results := FullAnalysis{}

	startTime := time.Now()
	color.Cyan("Running comprehensive analysis on %d files...\n", len(files))

	svc := analysis.New()

	// 1. Complexity
	if !excludeSet["complexity"] {
		tracker := progress.NewTracker("Analyzing complexity...", len(files))
		results.Complexity, _ = svc.AnalyzeComplexity(files, analysis.ComplexityOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	// 2. SATD
	if !excludeSet["satd"] {
		tracker := progress.NewTracker("Detecting technical debt...", len(files))
		results.SATD, _ = svc.AnalyzeSATD(files, analysis.SATDOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	// 3. Dead code
	if !excludeSet["deadcode"] {
		tracker := progress.NewTracker("Detecting dead code...", len(files))
		results.DeadCode, _ = svc.AnalyzeDeadCode(files, analysis.DeadCodeOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	// 4. Churn
	if !excludeSet["churn"] {
		spinner := progress.NewSpinner("Analyzing git churn...")
		results.Churn, _ = svc.AnalyzeChurn(context.Background(), repoPath, analysis.ChurnOptions{
			Spinner: spinner,
		})
		if results.Churn != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	// 5. Duplicates
	if !excludeSet["duplicates"] {
		tracker := progress.NewTracker("Detecting duplicates...", len(files))
		results.Clones, _ = svc.AnalyzeDuplicates(files, analysis.DuplicatesOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	// 6. Defect prediction (composite - uses sub-analyzers)
	if !excludeSet["defect"] {
		tracker := progress.NewTracker("Predicting defects...", 1)
		results.Defect, _ = svc.AnalyzeDefects(context.Background(), repoPath, files, analysis.DefectOptions{})
		tracker.FinishSuccess()
	}

	// 7. TDG
	if !excludeSet["tdg"] {
		tracker := progress.NewTracker("Calculating TDG scores...", 1)
		results.TDG, _ = svc.AnalyzeTDG(repoPath)
		tracker.FinishSuccess()
	}

	// 8. Hotspots (requires churn + complexity data)
	if !excludeSet["hotspots"] {
		spinner := progress.NewSpinner("Analyzing hotspots...")
		results.Hotspots, _ = svc.AnalyzeHotspots(context.Background(), repoPath, files, analysis.HotspotOptions{})
		if results.Hotspots != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	// 9. Architectural Smells
	if !excludeSet["smells"] {
		tracker := progress.NewTracker("Detecting architectural smells...", len(files))
		results.Smells, _ = svc.AnalyzeSmells(files, analysis.SmellOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	// 10. Ownership
	if !excludeSet["ownership"] {
		spinner := progress.NewSpinner("Analyzing code ownership...")
		results.Ownership, _ = svc.AnalyzeOwnership(repoPath, files, analysis.OwnershipOptions{})
		if results.Ownership != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	// 11. Temporal Coupling
	if !excludeSet["temporal-coupling"] {
		spinner := progress.NewSpinner("Analyzing temporal coupling...")
		results.TemporalCoupling, _ = svc.AnalyzeTemporalCoupling(context.Background(), repoPath, analysis.TemporalCouplingOptions{})
		if results.TemporalCoupling != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	// 12. Cohesion (CK metrics)
	if !excludeSet["cohesion"] {
		tracker := progress.NewTracker("Analyzing cohesion metrics...", len(files))
		results.Cohesion, _ = svc.AnalyzeCohesion(files, analysis.CohesionOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	// 13. Feature Flags
	if !excludeSet["flags"] {
		tracker := progress.NewTracker("Detecting feature flags...", len(files))
		results.FeatureFlags, _ = svc.AnalyzeFeatureFlags(files, analysis.FeatureFlagOptions{
			OnProgress: tracker.Tick,
			IncludeGit: true,
		})
		tracker.FinishSuccess()
	}

	elapsed := time.Since(startTime)
	fmt.Printf("\nAnalysis completed in %s\n\n", elapsed.Round(time.Millisecond))

	// For JSON/TOON, output raw results
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		return formatter.Output(results)
	}

	// Print summary report
	w := formatter.Writer()
	if formatter.Colored() {
		color.Cyan("=== Analysis Summary ===\n")
	} else {
		fmt.Fprintln(w, "=== Analysis Summary ===")
	}

	if results.Complexity != nil {
		fmt.Fprintf(w, "\nComplexity:\n")
		fmt.Fprintf(w, "  Files: %d, Functions: %d\n", results.Complexity.Summary.TotalFiles, results.Complexity.Summary.TotalFunctions)
		fmt.Fprintf(w, "  Median Cyclomatic (P50): %d, Median Cognitive (P50): %d\n", results.Complexity.Summary.P50Cyclomatic, results.Complexity.Summary.P50Cognitive)
		fmt.Fprintf(w, "  90th Percentile Cyclomatic: %d, 90th Percentile Cognitive: %d\n", results.Complexity.Summary.P90Cyclomatic, results.Complexity.Summary.P90Cognitive)
		fmt.Fprintf(w, "  Max Cyclomatic: %d, Max Cognitive: %d\n", results.Complexity.Summary.MaxCyclomatic, results.Complexity.Summary.MaxCognitive)
	}

	if results.SATD != nil {
		fmt.Fprintf(w, "\nTechnical Debt:\n")
		fmt.Fprintf(w, "  Total: %d items (High: %d, Medium: %d, Low: %d)\n",
			results.SATD.Summary.TotalItems,
			results.SATD.Summary.BySeverity["high"],
			results.SATD.Summary.BySeverity["medium"],
			results.SATD.Summary.BySeverity["low"])
	}

	if results.DeadCode != nil {
		fmt.Fprintf(w, "\nDead Code:\n")
		fmt.Fprintf(w, "  Functions: %d, Variables: %d (%.1f%% dead)\n",
			results.DeadCode.Summary.TotalDeadFunctions,
			results.DeadCode.Summary.TotalDeadVariables,
			results.DeadCode.Summary.DeadCodePercentage)
	}

	if results.Churn != nil {
		fmt.Fprintf(w, "\nFile Churn:\n")
		fmt.Fprintf(w, "  Files: %d, Total Commits: %d, Authors: %d\n",
			results.Churn.Summary.TotalFilesChanged,
			results.Churn.Summary.TotalCommits,
			len(results.Churn.Summary.AuthorContributions))
	}

	if results.Clones != nil {
		fmt.Fprintf(w, "\nCode Clones:\n")
		fmt.Fprintf(w, "  Total: %d (Type-1: %d, Type-2: %d, Type-3: %d)\n",
			results.Clones.Summary.TotalClones,
			results.Clones.Summary.Type1Count,
			results.Clones.Summary.Type2Count,
			results.Clones.Summary.Type3Count)
	}

	if results.Defect != nil {
		fmt.Fprintf(w, "\nDefect Prediction:\n")
		fmt.Fprintf(w, "  High Risk: %d, Medium Risk: %d, Low Risk: %d\n",
			results.Defect.Summary.HighRiskCount,
			results.Defect.Summary.MediumRiskCount,
			results.Defect.Summary.LowRiskCount)
		fmt.Fprintf(w, "  Avg Probability: %.0f%%\n", results.Defect.Summary.AvgProbability*100)
	}

	if results.TDG != nil {
		fmt.Fprintf(w, "\nTechnical Debt Gradient:\n")
		fmt.Fprintf(w, "  Files: %d, Avg Score: %.1f, Grade: %s\n",
			results.TDG.TotalFiles, results.TDG.AverageScore, results.TDG.AverageGrade)
		if len(results.TDG.Files) > 0 {
			// Find lowest score (worst file)
			worst := results.TDG.Files[0]
			for _, f := range results.TDG.Files {
				if f.Total < worst.Total {
					worst = f
				}
			}
			fmt.Fprintf(w, "  Lowest Score: %s (%.1f, %s)\n",
				worst.FilePath, worst.Total, worst.Grade)
		}
		// Display grade distribution
		if len(results.TDG.GradeDistribution) > 0 {
			fmt.Fprintf(w, "  Grade Distribution:\n")
			gradeOrder := []tdg.Grade{
				tdg.GradeAPlus, tdg.GradeA, tdg.GradeAMinus,
				tdg.GradeBPlus, tdg.GradeB, tdg.GradeBMinus,
				tdg.GradeCPlus, tdg.GradeC, tdg.GradeCMinus,
				tdg.GradeD, tdg.GradeF,
			}
			for _, grade := range gradeOrder {
				if count := results.TDG.GradeDistribution[grade]; count > 0 {
					percentage := float64(count) / float64(results.TDG.TotalFiles) * 100
					fmt.Fprintf(w, "    - %s: %d files (%.1f%%)\n", grade, count, percentage)
				}
			}
		}
	}

	if results.Hotspots != nil {
		fmt.Fprintf(w, "\nHotspots:\n")
		fmt.Fprintf(w, "  Files: %d, Hotspots (score >= 0.4): %d\n",
			results.Hotspots.Summary.TotalFiles,
			results.Hotspots.Summary.HotspotCount)
		fmt.Fprintf(w, "  P50 Score: %.2f, P90 Score: %.2f, Max: %.2f\n",
			results.Hotspots.Summary.P50HotspotScore,
			results.Hotspots.Summary.P90HotspotScore,
			results.Hotspots.Summary.MaxHotspotScore)
	}

	if results.Smells != nil {
		fmt.Fprintf(w, "\nArchitectural Smells:\n")
		fmt.Fprintf(w, "  Total: %d (Critical: %d, High: %d, Medium: %d)\n",
			results.Smells.Summary.TotalSmells,
			results.Smells.Summary.CriticalCount,
			results.Smells.Summary.HighCount,
			results.Smells.Summary.MediumCount)
		fmt.Fprintf(w, "  Cycles: %d, Hubs: %d, God Components: %d, Unstable: %d\n",
			results.Smells.Summary.CyclicCount,
			results.Smells.Summary.HubCount,
			results.Smells.Summary.GodCount,
			results.Smells.Summary.UnstableCount)
	}

	if results.Ownership != nil {
		fmt.Fprintf(w, "\nCode Ownership:\n")
		fmt.Fprintf(w, "  Files: %d, Bus Factor: %d, Silos: %d\n",
			results.Ownership.Summary.TotalFiles,
			results.Ownership.Summary.BusFactor,
			results.Ownership.Summary.SiloCount)
		fmt.Fprintf(w, "  Avg Contributors/File: %.1f\n", results.Ownership.Summary.AvgContributors)
	}

	if results.TemporalCoupling != nil {
		fmt.Fprintf(w, "\nTemporal Coupling:\n")
		fmt.Fprintf(w, "  Couplings: %d, Strong (>= 0.5): %d\n",
			results.TemporalCoupling.Summary.TotalCouplings,
			results.TemporalCoupling.Summary.StrongCouplings)
		fmt.Fprintf(w, "  Avg Strength: %.2f, Max: %.2f\n",
			results.TemporalCoupling.Summary.AvgCouplingStrength,
			results.TemporalCoupling.Summary.MaxCouplingStrength)
	}

	if results.Cohesion != nil {
		fmt.Fprintf(w, "\nCohesion (CK Metrics):\n")
		fmt.Fprintf(w, "  Classes: %d, Files: %d\n",
			results.Cohesion.Summary.TotalClasses,
			results.Cohesion.Summary.TotalFiles)
		fmt.Fprintf(w, "  Avg WMC: %.1f, Avg CBO: %.1f, Avg LCOM: %.1f\n",
			results.Cohesion.Summary.AvgWMC,
			results.Cohesion.Summary.AvgCBO,
			results.Cohesion.Summary.AvgLCOM)
		fmt.Fprintf(w, "  Low Cohesion (LCOM > 1): %d classes\n",
			results.Cohesion.Summary.LowCohesionCount)
	}

	if results.FeatureFlags != nil {
		fmt.Fprintf(w, "\nFeature Flags:\n")
		fmt.Fprintf(w, "  Flags: %d, References: %d\n",
			results.FeatureFlags.Summary.TotalFlags,
			results.FeatureFlags.Summary.TotalReferences)
		fmt.Fprintf(w, "  By Priority: Critical: %d, High: %d, Medium: %d, Low: %d\n",
			results.FeatureFlags.Summary.ByPriority["CRITICAL"],
			results.FeatureFlags.Summary.ByPriority["HIGH"],
			results.FeatureFlags.Summary.ByPriority["MEDIUM"],
			results.FeatureFlags.Summary.ByPriority["LOW"])
		fmt.Fprintf(w, "  Avg File Spread: %.1f, Max: %d\n",
			results.FeatureFlags.Summary.AvgFileSpread,
			results.FeatureFlags.Summary.MaxFileSpread)
	}

	return nil
}

func mcpCmd() *cli.Command {
	return &cli.Command{
		Name:  "mcp",
		Usage: "Start MCP (Model Context Protocol) server for LLM tool integration",
		Description: `Starts an MCP server over stdio transport that exposes omen's analyzers
as tools that LLMs can invoke. This enables AI assistants like Claude to
analyze codebases for complexity, technical debt, dead code, and more.

To use with Claude Desktop, add to your config:
  {
    "mcpServers": {
      "omen": {
        "command": "omen",
        "args": ["mcp"]
      }
    }
  }

Available tools:
  - analyze_complexity    Cyclomatic and cognitive complexity
  - analyze_satd          Self-admitted technical debt (TODO/FIXME/HACK)
  - analyze_deadcode      Unused functions and variables
  - analyze_churn         Git file change frequency
  - analyze_duplicates    Code clones and copy-paste detection
  - analyze_defect        Defect probability prediction
  - analyze_tdg           Technical Debt Gradient scores
  - analyze_graph         Dependency graph generation
  - analyze_hotspot       High churn + high complexity files
  - analyze_temporal_coupling  Files that change together
  - analyze_ownership     Code ownership and bus factor
  - analyze_cohesion      CK OO metrics (LCOM, WMC, CBO, DIT)
  - analyze_repo_map      PageRank-ranked symbol map`,
		Subcommands: []*cli.Command{
			{
				Name:  "manifest",
				Usage: "Output MCP server manifest (server.json) for registry publishing",
				Action: func(c *cli.Context) error {
					data, err := mcpserver.GenerateManifest(version)
					if err != nil {
						return err
					}
					fmt.Println(string(data))
					return nil
				},
			},
		},
		Action: func(c *cli.Context) error {
			server := mcpserver.NewServer(version)
			return server.Run(context.Background())
		},
	}
}

func initCmd() *cli.Command {
	return &cli.Command{
		Name:  "init",
		Usage: "Initialize a new omen configuration file",
		Description: `Creates a new omen.toml configuration file in the current directory
with sensible defaults. Use --output to specify a different location.

Examples:
  omen init                    # Creates omen.toml in current directory
  omen init -o .omen/omen.toml # Creates config in .omen directory
  omen init --force            # Overwrite existing config file`,
		Flags: []cli.Flag{
			&cli.StringFlag{
				Name:    "output",
				Aliases: []string{"o"},
				Value:   "omen.toml",
				Usage:   "Output file path",
			},
			&cli.BoolFlag{
				Name:  "force",
				Usage: "Overwrite existing config file",
			},
		},
		Action: runInitCmd,
	}
}

func runInitCmd(c *cli.Context) error {
	outputPath := c.String("output")
	force := c.Bool("force")

	// Check if file already exists
	if _, err := os.Stat(outputPath); err == nil && !force {
		return fmt.Errorf("config file %q already exists (use --force to overwrite)", outputPath)
	}

	// Create parent directory if needed
	dir := filepath.Dir(outputPath)
	if dir != "" && dir != "." {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return fmt.Errorf("failed to create directory %q: %w", dir, err)
		}
	}

	// Generate default config content
	content, err := generateDefaultConfig()
	if err != nil {
		return err
	}

	if err := os.WriteFile(outputPath, []byte(content), 0o644); err != nil {
		return fmt.Errorf("failed to write config file: %w", err)
	}

	color.Green("Created %s", outputPath)
	fmt.Println("Edit this file to customize analysis settings.")
	return nil
}

func generateDefaultConfig() (string, error) {
	cfg := config.DefaultConfig()

	content, err := toml.Marshal(cfg)
	if err != nil {
		return "", fmt.Errorf("failed to marshal config to TOML: %w", err)
	}

	var buf strings.Builder
	buf.WriteString("# Omen CLI Configuration\n")
	buf.WriteString("# Documentation: https://github.com/panbanda/omen\n\n")
	buf.Write(content)

	return buf.String(), nil
}

func configCmd() *cli.Command {
	return &cli.Command{
		Name:  "config",
		Usage: "Configuration management commands",
		Subcommands: []*cli.Command{
			{
				Name:  "validate",
				Usage: "Validate a configuration file",
				Description: `Validates an omen configuration file for syntax errors and invalid values.

Examples:
  omen config validate                  # Validates default config locations
  omen config validate -c omen.toml     # Validates specific file
  omen config validate -c .omen/omen.toml`,
				Flags: []cli.Flag{
					&cli.StringFlag{
						Name:    "config",
						Aliases: []string{"c"},
						Usage:   "Path to config file to validate",
					},
				},
				Action: runConfigValidateCmd,
			},
			{
				Name:  "show",
				Usage: "Show the effective configuration",
				Description: `Shows the merged configuration from defaults and config file.

Examples:
  omen config show              # Show effective config
  omen config show -c omen.toml # Show config from specific file`,
				Flags: []cli.Flag{
					&cli.StringFlag{
						Name:    "config",
						Aliases: []string{"c"},
						Usage:   "Path to config file",
					},
				},
				Action: runConfigShowCmd,
			},
		},
	}
}

func runConfigValidateCmd(c *cli.Context) error {
	var opts []config.LoadOption
	if path := c.String("config"); path != "" {
		opts = append(opts, config.WithPath(path))
	}

	result, err := config.LoadConfig(opts...)
	if err != nil {
		color.Red("Configuration validation failed:")
		fmt.Printf("  - %s\n", err)
		return err
	}

	if result.Source != "" {
		color.Green("Configuration valid: %s", result.Source)
	} else {
		color.Yellow("No config file found. Default configuration is valid.")
	}
	return nil
}

func runConfigShowCmd(c *cli.Context) error {
	var opts []config.LoadOption
	if path := c.String("config"); path != "" {
		opts = append(opts, config.WithPath(path))
	}

	result, err := config.LoadConfig(opts...)
	if err != nil {
		return err
	}

	if result.Source != "" {
		fmt.Printf("# Configuration from: %s\n\n", result.Source)
	} else {
		fmt.Println("# Default configuration (no config file found)")
	}

	content, err := toml.Marshal(result.Config)
	if err != nil {
		return fmt.Errorf("failed to marshal config: %w", err)
	}
	fmt.Print(string(content))

	return nil
}

func scoreCmd() *cli.Command {
	flags := append(outputFlags(),
		&cli.IntFlag{
			Name:  "min-score",
			Usage: "Minimum composite score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-complexity",
			Usage: "Minimum complexity score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-duplication",
			Usage: "Minimum duplication score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-defect",
			Usage: "Minimum defect risk score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-debt",
			Usage: "Minimum technical debt score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-coupling",
			Usage: "Minimum coupling score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-smells",
			Usage: "Minimum smells score (0-100)",
		},
		&cli.IntFlag{
			Name:  "min-cohesion",
			Usage: "Minimum cohesion score (0-100)",
		},
		&cli.BoolFlag{
			Name:  "enable-cohesion",
			Usage: "Include cohesion in composite score (redistributes weights)",
		},
		&cli.IntFlag{
			Name:  "days",
			Value: 30,
			Usage: "Days of git history for churn analysis",
		},
	)
	return &cli.Command{
		Name:      "score",
		Usage:     "Compute repository health score",
		ArgsUsage: "[path...]",
		Flags:     flags,
		Action:    runScoreCmd,
	}
}

func runScoreCmd(c *cli.Context) error {
	paths := getPaths(c)

	cfg, err := config.LoadOrDefault()
	if err != nil {
		return err
	}

	// CLI flag overrides config
	if c.Bool("enable-cohesion") {
		cfg.Score.EnableCohesion = true
	}

	// Build thresholds from flags (override config)
	thresholds := score.Thresholds{
		Score:       intOrDefault(c.Int("min-score"), cfg.Score.Thresholds.Score),
		Complexity:  intOrDefault(c.Int("min-complexity"), cfg.Score.Thresholds.Complexity),
		Duplication: intOrDefault(c.Int("min-duplication"), cfg.Score.Thresholds.Duplication),
		Defect:      intOrDefault(c.Int("min-defect"), cfg.Score.Thresholds.Defect),
		Debt:        intOrDefault(c.Int("min-debt"), cfg.Score.Thresholds.Debt),
		Coupling:    intOrDefault(c.Int("min-coupling"), cfg.Score.Thresholds.Coupling),
		Smells:      intOrDefault(c.Int("min-smells"), cfg.Score.Thresholds.Smells),
		Cohesion:    intOrDefault(c.Int("min-cohesion"), cfg.Score.Thresholds.Cohesion),
	}

	// Build weights from effective config (handles enable_cohesion)
	effectiveWeights := cfg.Score.EffectiveWeights()
	weights := score.Weights{
		Complexity:  effectiveWeights.Complexity,
		Duplication: effectiveWeights.Duplication,
		Defect:      effectiveWeights.Defect,
		Debt:        effectiveWeights.Debt,
		Coupling:    effectiveWeights.Coupling,
		Smells:      effectiveWeights.Smells,
		Cohesion:    effectiveWeights.Cohesion,
	}

	scanSvc := scannerSvc.New()
	scanResult, err := scanSvc.ScanPaths(paths)
	if err != nil {
		return err
	}

	if len(scanResult.Files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	// Find repo root
	repoPath := "."
	if len(paths) > 0 {
		repoPath = paths[0]
	}

	tracker := progress.NewTracker("Computing repository score...", 7)

	analyzer := score.New(
		score.WithWeights(weights),
		score.WithThresholds(thresholds),
		score.WithChurnDays(c.Int("days")),
		score.WithMaxFileSize(cfg.Analysis.MaxFileSize),
	)

	result, err := analyzer.AnalyzeProjectWithProgress(context.Background(), repoPath, scanResult.Files, func(stage string) {
		tracker.Tick()
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("score analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Output based on format
	format := getFormat(c)
	if format == "json" || format == "toon" {
		if err := formatter.Output(result); err != nil {
			return err
		}
	} else if format == "markdown" {
		if err := writeScoreMarkdown(result, formatter); err != nil {
			return err
		}
	} else {
		printScoreResult(result)
	}

	// Exit with error if thresholds not met
	if !result.Passed {
		return cli.Exit("threshold violation", 1)
	}
	return nil
}

func intOrDefault(flag, cfg int) int {
	if flag != 0 {
		return flag
	}
	return cfg
}

func printScoreResult(r *score.Result) {
	// Header with score coloring
	scoreColor := color.New(color.FgGreen)
	if r.Score < 70 {
		scoreColor = color.New(color.FgYellow)
	}
	if r.Score < 50 {
		scoreColor = color.New(color.FgRed)
	}

	fmt.Println()
	scoreColor.Printf("Repository Score: %d/100\n", r.Score)
	fmt.Println()

	// Components
	printScoreComponent("Complexity", r.Components.Complexity)
	printScoreComponent("Duplication", r.Components.Duplication)
	printScoreComponent("Defect Risk", r.Components.Defect)
	printScoreComponent("Technical Debt", r.Components.Debt)
	printScoreComponent("Coupling", r.Components.Coupling)
	printScoreComponent("Smells", r.Components.Smells)
	if r.CohesionIncluded {
		printScoreComponent("Cohesion", r.Components.Cohesion)
	}
	fmt.Println()
	fmt.Printf("Files analyzed: %d\n", r.FilesAnalyzed)

	// Threshold failures
	if !r.Passed {
		fmt.Println()
		color.Red("Threshold violations:")
		for name, th := range r.Thresholds {
			if !th.Passed {
				actual := getScoreComponent(r, name)
				color.Red("  %s: %d < %d (minimum)", name, actual, th.Min)
			}
		}
		fmt.Println()
		fmt.Println("Run analyzers to diagnose:")
		for name, th := range r.Thresholds {
			if !th.Passed && name != "score" {
				fmt.Printf("  omen analyze %s\n", componentToAnalyzer(name))
			}
		}
	}
}

func componentToAnalyzer(name string) string {
	switch name {
	case "complexity":
		return "complexity"
	case "duplication":
		return "duplicates"
	case "defect":
		return "defect"
	case "debt":
		return "satd"
	case "coupling":
		return "graph"
	case "smells":
		return "smells"
	case "cohesion":
		return "cohesion"
	default:
		return name
	}
}

func printScoreComponent(name string, value int) {
	c := color.New(color.FgGreen)
	if value < 70 {
		c = color.New(color.FgYellow)
	}
	if value < 50 {
		c = color.New(color.FgRed)
	}
	c.Printf("  %-15s %3d/100\n", name+":", value)
}

func getScoreComponent(r *score.Result, name string) int {
	switch name {
	case "score":
		return r.Score
	case "complexity":
		return r.Components.Complexity
	case "duplication":
		return r.Components.Duplication
	case "defect":
		return r.Components.Defect
	case "debt":
		return r.Components.Debt
	case "coupling":
		return r.Components.Coupling
	case "smells":
		return r.Components.Smells
	case "cohesion":
		return r.Components.Cohesion
	default:
		return 0
	}
}

func writeScoreMarkdown(r *score.Result, f *output.Formatter) error {
	w := f.Writer()
	fmt.Fprintf(w, "# Repository Score: %d/100\n\n", r.Score)
	fmt.Fprintln(w, "## Component Scores")
	fmt.Fprintln(w)
	fmt.Fprintln(w, "| Component | Score |")
	fmt.Fprintln(w, "|-----------|-------|")
	fmt.Fprintf(w, "| Complexity | %d/100 |\n", r.Components.Complexity)
	fmt.Fprintf(w, "| Duplication | %d/100 |\n", r.Components.Duplication)
	fmt.Fprintf(w, "| Defect Risk | %d/100 |\n", r.Components.Defect)
	fmt.Fprintf(w, "| Technical Debt | %d/100 |\n", r.Components.Debt)
	fmt.Fprintf(w, "| Coupling | %d/100 |\n", r.Components.Coupling)
	fmt.Fprintf(w, "| Smells | %d/100 |\n", r.Components.Smells)
	if r.CohesionIncluded {
		fmt.Fprintf(w, "| Cohesion | %d/100 |\n", r.Components.Cohesion)
	}
	fmt.Fprintf(w, "\nFiles analyzed: %d\n", r.FilesAnalyzed)
	if r.Commit != "" {
		fmt.Fprintf(w, "Commit: %s\n", r.Commit)
	}
	fmt.Fprintf(w, "Timestamp: %s\n", r.Timestamp.Format(time.RFC3339))

	if !r.Passed {
		fmt.Fprintln(w)
		fmt.Fprintln(w, "## Threshold Violations")
		fmt.Fprintln(w)
		for name, th := range r.Thresholds {
			if !th.Passed {
				actual := getScoreComponent(r, name)
				fmt.Fprintf(w, "- **%s**: %d < %d (minimum)\n", name, actual, th.Min)
			}
		}
	}

	return nil
}
