package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"runtime/pprof"
	"sort"
	"strings"
	"time"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/analyzer"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/scanner"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/models"
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
	if days > 3650 { // ~10 years
		return fmt.Errorf("--days cannot exceed 3650 (10 years), got %d", days)
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
			analyzeCmd(),
			contextCmd(),
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
		&cli.BoolFlag{
			Name:  "halstead",
			Usage: "Include Halstead software science metrics",
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
	includeHalstead := c.Bool("halstead")

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	opts := analyzer.ComplexityOptions{
		IncludeHalstead: includeHalstead,
	}
	cxAnalyzer := analyzer.NewComplexityAnalyzerWithOptions(opts)
	defer cxAnalyzer.Close()

	tracker := progress.NewTracker("Analyzing complexity...", len(files))
	analysis, err := cxAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
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

	for _, fc := range analysis.Files {
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
			fmt.Sprintf("Files: %d", analysis.Summary.TotalFiles),
			fmt.Sprintf("Functions: %d", analysis.Summary.TotalFunctions),
			fmt.Sprintf("Median Cyclomatic (P50): %d", analysis.Summary.P50Cyclomatic),
			fmt.Sprintf("Median Cognitive (P50): %d", analysis.Summary.P50Cognitive),
			fmt.Sprintf("90th Percentile Cyclomatic: %d", analysis.Summary.P90Cyclomatic),
			fmt.Sprintf("90th Percentile Cognitive: %d", analysis.Summary.P90Cognitive),
			fmt.Sprintf("Max Cyclomatic: %d", analysis.Summary.MaxCyclomatic),
			fmt.Sprintf("Max Cognitive: %d", analysis.Summary.MaxCognitive),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	if includeTest {
		cfg.Exclude.Patterns = []string{} // Clear test exclusions
	}
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	opts := analyzer.SATDOptions{
		IncludeTests:      includeTest,
		IncludeVendor:     false,
		AdjustSeverity:    true,
		GenerateContextID: true,
	}
	satdAnalyzer := analyzer.NewSATDAnalyzerWithOptions(opts)
	for _, p := range patterns {
		if err := satdAnalyzer.AddPattern(p, models.DebtDesign, models.SeverityMedium); err != nil {
			color.Yellow("Invalid pattern %q: %v", p, err)
		}
	}

	tracker := progress.NewTracker("Detecting technical debt...", len(files))
	analysis, err := satdAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
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
	for _, item := range analysis.Items {
		sevColor := string(item.Severity)
		switch item.Severity {
		case models.SeverityHigh:
			sevColor = color.RedString(string(item.Severity))
		case models.SeverityMedium:
			sevColor = color.YellowString(string(item.Severity))
		case models.SeverityLow:
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
			fmt.Sprintf("Total: %d", analysis.Summary.TotalItems),
			fmt.Sprintf("High: %d", analysis.Summary.BySeverity["high"]),
			fmt.Sprintf("Medium: %d", analysis.Summary.BySeverity["medium"]),
			fmt.Sprintf("Low: %d", analysis.Summary.BySeverity["low"]),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	dcAnalyzer := analyzer.NewDeadCodeAnalyzer(confidence)
	defer dcAnalyzer.Close()

	tracker := progress.NewTracker("Detecting dead code...", len(files))
	analysis, err := dcAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
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
		result := models.NewDeadCodeResult()
		result.FromDeadCodeAnalysis(analysis)
		return formatter.Output(result)
	}

	// Functions table
	if len(analysis.DeadFunctions) > 0 {
		var rows [][]string
		for _, fn := range analysis.DeadFunctions {
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
	if len(analysis.DeadVariables) > 0 {
		var rows [][]string
		for _, v := range analysis.DeadVariables {
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
		analysis.Summary.TotalDeadFunctions,
		analysis.Summary.TotalDeadVariables,
		analysis.Summary.TotalFilesAnalyzed,
		analysis.Summary.DeadCodePercentage)

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

	churnAnalyzer := analyzer.NewChurnAnalyzer(days)
	spinner := progress.NewSpinner("Analyzing git history...")
	churnAnalyzer.SetSpinner(spinner)
	analysis, err := churnAnalyzer.AnalyzeRepo(absPath)
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
	files := analysis.Files
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
			fmt.Sprintf("Total Files: %d", analysis.Summary.TotalFilesChanged),
			fmt.Sprintf("Total Commits: %d", analysis.Summary.TotalCommits),
			fmt.Sprintf("Authors: %d", len(analysis.Summary.AuthorContributions)),
			"",
			fmt.Sprintf("Max: %.2f", analysis.Summary.MaxChurnScore),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	dupAnalyzer := analyzer.NewDuplicateAnalyzer(minLines, threshold)
	defer dupAnalyzer.Close()

	tracker := progress.NewTracker("Detecting duplicates...", len(files))
	analysis, err := dupAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
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
		report := analysis.ToCloneReport()
		return formatter.Output(report)
	}

	if len(analysis.Clones) == 0 {
		if formatter.Format() == output.FormatText {
			color.Green("No code duplicates found above %.0f%% similarity threshold", threshold*100)
		}
		return formatter.Output(analysis)
	}

	var rows [][]string
	for _, clone := range analysis.Clones {
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
			fmt.Sprintf("Total Clones: %d", analysis.Summary.TotalClones),
			fmt.Sprintf("Type-1: %d", analysis.Summary.Type1Count),
			fmt.Sprintf("Type-2: %d", analysis.Summary.Type2Count),
			fmt.Sprintf("Type-3: %d", analysis.Summary.Type3Count),
			fmt.Sprintf("Avg Sim: %.0f%%", analysis.Summary.AvgSimilarity*100),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	// Use first path as repo root for git analysis
	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	defectAnalyzer := analyzer.NewDefectAnalyzer(cfg.Analysis.ChurnDays)
	defer defectAnalyzer.Close()

	analysis, err := defectAnalyzer.AnalyzeProject(repoPath, files)
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Sort by probability (highest first)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].Probability > analysis.Files[j].Probability
	})

	// For JSON/TOON, output pmat-compatible format
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		report := analysis.ToDefectPredictionReport()
		return formatter.Output(report)
	}

	var rows [][]string
	for _, ds := range analysis.Files {
		if highRiskOnly && ds.RiskLevel != models.RiskHigh {
			continue
		}

		probStr := fmt.Sprintf("%.0f%%", ds.Probability*100)
		riskStr := string(ds.RiskLevel)
		switch ds.RiskLevel {
		case models.RiskHigh:
			probStr = color.RedString(probStr)
			riskStr = color.RedString(riskStr)
		case models.RiskMedium:
			probStr = color.YellowString(probStr)
			riskStr = color.YellowString(riskStr)
		case models.RiskLow:
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
			fmt.Sprintf("Total Files: %d", analysis.Summary.TotalFiles),
			fmt.Sprintf("High Risk: %d", analysis.Summary.HighRiskCount),
			fmt.Sprintf("Medium Risk: %d", analysis.Summary.MediumRiskCount),
			fmt.Sprintf("Avg Prob: %.0f%%", analysis.Summary.AvgProbability*100),
		},
		analysis,
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

	tdgAnalyzer := analyzer.NewTdgAnalyzer()
	defer tdgAnalyzer.Close()

	// Handle directory vs single file
	var project models.ProjectScore
	var err error

	if len(paths) == 1 {
		info, statErr := os.Stat(paths[0])
		if statErr != nil {
			return fmt.Errorf("invalid path %s: %w", paths[0], statErr)
		}

		if info.IsDir() {
			project, err = tdgAnalyzer.AnalyzeProject(paths[0])
		} else {
			score, fileErr := tdgAnalyzer.AnalyzeFile(paths[0])
			if fileErr != nil {
				return fmt.Errorf("analysis failed: %w", fileErr)
			}
			project = models.AggregateProjectScore([]models.TdgScore{score})
		}
	} else {
		var allScores []models.TdgScore
		for _, path := range paths {
			info, statErr := os.Stat(path)
			if statErr != nil {
				return fmt.Errorf("invalid path %s: %w", path, statErr)
			}

			if info.IsDir() {
				dirProject, dirErr := tdgAnalyzer.AnalyzeProject(path)
				if dirErr != nil {
					return fmt.Errorf("analysis failed for %s: %w", path, dirErr)
				}
				allScores = append(allScores, dirProject.Files...)
			} else {
				score, fileErr := tdgAnalyzer.AnalyzeFile(path)
				if fileErr != nil {
					return fmt.Errorf("analysis failed for %s: %w", path, fileErr)
				}
				allScores = append(allScores, score)
			}
		}
		project = models.AggregateProjectScore(allScores)
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
		report := project.ToTDGReport(hotspots)
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
		gradeOrder := []models.Grade{
			models.GradeAPlus,
			models.GradeA,
			models.GradeAMinus,
			models.GradeBPlus,
			models.GradeB,
			models.GradeBMinus,
			models.GradeCPlus,
			models.GradeC,
			models.GradeCMinus,
			models.GradeD,
			models.GradeF,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	graphAnalyzer := analyzer.NewGraphAnalyzer(analyzer.GraphScope(scope))
	defer graphAnalyzer.Close()

	tracker := progress.NewTracker("Building dependency graph...", len(files))
	graph, err := graphAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
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
		if includeMetrics {
			metrics := graphAnalyzer.CalculateMetrics(graph)
			return formatter.Output(struct {
				Graph   *models.DependencyGraph `json:"graph" toon:"graph"`
				Metrics *models.GraphMetrics    `json:"metrics" toon:"metrics"`
			}{graph, metrics})
		}
		return formatter.Output(graph)
	}

	// Generate Mermaid diagram for text/markdown
	fmt.Fprintln(formatter.Writer(), "```mermaid")
	fmt.Fprintln(formatter.Writer(), "graph TD")
	for _, node := range graph.Nodes {
		fmt.Fprintf(formatter.Writer(), "    %s[%s]\n", sanitizeID(node.ID), node.Name)
	}
	for _, edge := range graph.Edges {
		fmt.Fprintf(formatter.Writer(), "    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
	}
	fmt.Fprintln(formatter.Writer(), "```")

	if includeMetrics {
		metrics := graphAnalyzer.CalculateMetrics(graph)
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	// Use complexity as hotspot indicator
	cxAnalyzer := analyzer.NewComplexityAnalyzer()
	defer cxAnalyzer.Close()

	tracker := progress.NewTracker("Analyzing hotspots...", len(files))
	analysis, err := cxAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
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
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].TotalCyclomatic+analysis.Files[i].TotalCognitive >
			analysis.Files[j].TotalCyclomatic+analysis.Files[j].TotalCognitive
	})

	filesToShow := analysis.Files
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
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	hotspotAnalyzer := analyzer.NewHotspotAnalyzer(days)
	defer hotspotAnalyzer.Close()

	tracker := progress.NewTracker("Analyzing hotspots...", len(files))
	analysis, err := hotspotAnalyzer.AnalyzeProjectWithProgress(repoPath, files, tracker.Tick)
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
	filesToShow := analysis.Files
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
			fmt.Sprintf("Total Files: %d", analysis.Summary.TotalFiles),
			fmt.Sprintf("Hotspots (>0.5): %d", analysis.Summary.HotspotCount),
			fmt.Sprintf("Max Score: %.2f", analysis.Summary.MaxHotspotScore),
			fmt.Sprintf("Avg Score: %.2f", analysis.Summary.AvgHotspotScore),
		},
		analysis,
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

	tcAnalyzer := analyzer.NewTemporalCouplingAnalyzer(days, minCochanges)
	defer tcAnalyzer.Close()

	spinner := progress.NewSpinner("Analyzing temporal coupling...")
	analysis, err := tcAnalyzer.AnalyzeRepo(repoPath)
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
	couplingsToShow := analysis.Couplings
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
			fmt.Sprintf("Total Couplings: %d", analysis.Summary.TotalCouplings),
			fmt.Sprintf("Strong (>0.5): %d", analysis.Summary.StrongCouplings),
			fmt.Sprintf("Files Analyzed: %d", analysis.Summary.TotalFilesAnalyzed),
			fmt.Sprintf("Max Strength: %.2f", analysis.Summary.MaxCouplingStrength),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	ownAnalyzer := analyzer.NewOwnershipAnalyzerWithOptions(!includeTrivial)
	defer ownAnalyzer.Close()

	tracker := progress.NewTracker("Analyzing ownership", len(files))
	analysis, err := ownAnalyzer.AnalyzeRepoWithProgress(repoPath, files, func() {
		tracker.Tick()
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
	filesToShow := analysis.Files
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
			fmt.Sprintf("Total Files: %d", analysis.Summary.TotalFiles),
			fmt.Sprintf("Bus Factor: %d", analysis.Summary.BusFactor),
			fmt.Sprintf("Silos: %d", analysis.Summary.SiloCount),
			fmt.Sprintf("Avg Contributors: %.1f", analysis.Summary.AvgContributors),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	ckAnalyzer := analyzer.NewCohesionAnalyzerWithOptions(!includeTests)
	defer ckAnalyzer.Close()

	tracker := progress.NewTracker("Analyzing CK metrics", len(files))
	analysis, err := ckAnalyzer.AnalyzeProjectWithProgress(files, func() {
		tracker.Tick()
	})
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("cohesion analysis failed: %w", err)
	}

	if len(analysis.Classes) == 0 {
		color.Yellow("No OO classes found (CK metrics only apply to Java, Python, TypeScript, etc.)")
		return nil
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

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(c)), getOutputFile(c), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Limit results for display
	classesToShow := analysis.Classes
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
			fmt.Sprintf("Total Classes: %d", analysis.Summary.TotalClasses),
			fmt.Sprintf("Low Cohesion (LCOM>1): %d", analysis.Summary.LowCohesionCount),
			fmt.Sprintf("Avg WMC: %.1f", analysis.Summary.AvgWMC),
			fmt.Sprintf("Max WMC: %d", analysis.Summary.MaxWMC),
			fmt.Sprintf("Max DIT: %d", analysis.Summary.MaxDIT),
		},
		analysis,
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

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

	// Group by language
	langGroups := scan.GroupByLanguage(files)

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
		cxAnalyzer := analyzer.NewComplexityAnalyzer()
		defer cxAnalyzer.Close()

		analysis, err := cxAnalyzer.AnalyzeProject(files)
		if err == nil {
			fmt.Printf("- **Total Functions**: %d\n", analysis.Summary.TotalFunctions)
			fmt.Printf("- **Median Cyclomatic (P50)**: %d\n", analysis.Summary.P50Cyclomatic)
			fmt.Printf("- **Median Cognitive (P50)**: %d\n", analysis.Summary.P50Cognitive)
			fmt.Printf("- **90th Percentile Cyclomatic**: %d\n", analysis.Summary.P90Cyclomatic)
			fmt.Printf("- **90th Percentile Cognitive**: %d\n", analysis.Summary.P90Cognitive)
			fmt.Printf("- **Max Cyclomatic**: %d\n", analysis.Summary.MaxCyclomatic)
			fmt.Printf("- **Max Cognitive**: %d\n", analysis.Summary.MaxCognitive)
		}
	}

	if includeGraph {
		fmt.Println()
		fmt.Println("## Dependency Graph")
		graphAnalyzer := analyzer.NewGraphAnalyzer(analyzer.ScopeFile)
		defer graphAnalyzer.Close()

		graph, err := graphAnalyzer.AnalyzeProject(files)
		if err == nil {
			fmt.Println("```mermaid")
			fmt.Println("graph TD")
			for _, node := range graph.Nodes {
				fmt.Printf("    %s[%s]\n", sanitizeID(node.ID), node.Name)
			}
			for _, edge := range graph.Edges {
				fmt.Printf("    %s --> %s\n", sanitizeID(edge.From), sanitizeID(edge.To))
			}
			fmt.Println("```")
		}
	}

	if repoMap {
		fmt.Println()
		fmt.Println("## Repository Map")
		rmAnalyzer := analyzer.NewRepoMapAnalyzer()
		defer rmAnalyzer.Close()

		spinner := progress.NewSpinner("Generating repo map...")
		rm, err := rmAnalyzer.AnalyzeProject(files)
		spinner.FinishSuccess()

		if err == nil {
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
			tdgCmd(),
			graphCmd(),
			lintHotspotCmd(),
			hotspotCmd(),
			temporalCouplingCmd(),
			ownershipCmd(),
			cohesionCmd(),
		},
	}
}

func runAnalyzeCmd(c *cli.Context) error {
	paths := getPaths(c)
	exclude := c.StringSlice("exclude")

	cfg := config.LoadOrDefault()
	scan := scanner.NewScanner(cfg)

	// Use first path as repo root for git-based analyzers
	repoPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	var files []string
	for _, path := range paths {
		absPath, err := filepath.Abs(path)
		if err != nil {
			return fmt.Errorf("invalid path %s: %w", path, err)
		}
		found, err := scan.ScanDir(absPath)
		if err != nil {
			return fmt.Errorf("failed to scan directory %s: %w", path, err)
		}
		files = append(files, found...)
	}

	if len(files) == 0 {
		color.Yellow("No source files found")
		return nil
	}

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
		Complexity *models.ComplexityAnalysis `json:"complexity,omitempty"`
		SATD       *models.SATDAnalysis       `json:"satd,omitempty"`
		DeadCode   *models.DeadCodeAnalysis   `json:"dead_code,omitempty"`
		Churn      *models.ChurnAnalysis      `json:"churn,omitempty"`
		Clones     *models.CloneAnalysis      `json:"clones,omitempty"`
		Defect     *models.DefectAnalysis     `json:"defect,omitempty"`
		TDG        *models.ProjectScore       `json:"tdg,omitempty"`
	}
	results := FullAnalysis{}

	startTime := time.Now()
	color.Cyan("Running comprehensive analysis on %d files...\n", len(files))

	// 1. Complexity
	if !excludeSet["complexity"] {
		tracker := progress.NewTracker("Analyzing complexity...", len(files))
		cxAnalyzer := analyzer.NewComplexityAnalyzer()
		results.Complexity, _ = cxAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
		cxAnalyzer.Close()
		tracker.FinishSuccess()
	}

	// 2. SATD
	if !excludeSet["satd"] {
		tracker := progress.NewTracker("Detecting technical debt...", len(files))
		satdAnalyzer := analyzer.NewSATDAnalyzer()
		results.SATD, _ = satdAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
		tracker.FinishSuccess()
	}

	// 3. Dead code
	if !excludeSet["deadcode"] {
		tracker := progress.NewTracker("Detecting dead code...", len(files))
		dcAnalyzer := analyzer.NewDeadCodeAnalyzer(cfg.Thresholds.DeadCodeConfidence)
		results.DeadCode, _ = dcAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
		dcAnalyzer.Close()
		tracker.FinishSuccess()
	}

	// 4. Churn
	if !excludeSet["churn"] {
		spinner := progress.NewSpinner("Analyzing git churn...")
		churnAnalyzer := analyzer.NewChurnAnalyzer(cfg.Analysis.ChurnDays)
		churnAnalyzer.SetSpinner(spinner)
		results.Churn, _ = churnAnalyzer.AnalyzeRepo(repoPath)
		if results.Churn != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	// 5. Duplicates
	if !excludeSet["duplicates"] {
		tracker := progress.NewTracker("Detecting duplicates...", len(files))
		dupAnalyzer := analyzer.NewDuplicateAnalyzer(cfg.Thresholds.DuplicateMinLines, cfg.Thresholds.DuplicateSimilarity)
		results.Clones, _ = dupAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
		dupAnalyzer.Close()
		tracker.FinishSuccess()
	}

	// 6. Defect prediction (composite - uses sub-analyzers)
	if !excludeSet["defect"] {
		tracker := progress.NewTracker("Predicting defects...", 1)
		defectAnalyzer := analyzer.NewDefectAnalyzer(cfg.Analysis.ChurnDays)
		results.Defect, _ = defectAnalyzer.AnalyzeProject(repoPath, files)
		defectAnalyzer.Close()
		tracker.FinishSuccess()
	}

	// 7. TDG
	if !excludeSet["tdg"] {
		tracker := progress.NewTracker("Calculating TDG scores...", 1)
		tdgAnalyzer := analyzer.NewTdgAnalyzer()
		project, _ := tdgAnalyzer.AnalyzeProject(repoPath)
		results.TDG = &project
		tdgAnalyzer.Close()
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
		fmt.Fprintf(w, "\nFile Churn (last %d days):\n", cfg.Analysis.ChurnDays)
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
			gradeOrder := []models.Grade{
				models.GradeAPlus, models.GradeA, models.GradeAMinus,
				models.GradeBPlus, models.GradeB, models.GradeBMinus,
				models.GradeCPlus, models.GradeC, models.GradeCMinus,
				models.GradeD, models.GradeF,
			}
			for _, grade := range gradeOrder {
				if count := results.TDG.GradeDistribution[grade]; count > 0 {
					percentage := float64(count) / float64(results.TDG.TotalFiles) * 100
					fmt.Fprintf(w, "    - %s: %d files (%.1f%%)\n", grade, count, percentage)
				}
			}
		}
	}

	return nil
}
