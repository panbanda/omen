package main

import (
	"context"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"runtime/pprof"
	"sort"
	"syscall"
	"time"

	"github.com/fatih/color"
	"github.com/panbanda/omen/pkg/analyzer"
	"github.com/panbanda/omen/pkg/config"
	"github.com/panbanda/omen/pkg/models"
	"github.com/panbanda/omen/pkg/output"
	"github.com/panbanda/omen/pkg/progress"
	"github.com/panbanda/omen/pkg/scanner"
	"github.com/panbanda/omen/pkg/watch"
	"github.com/urfave/cli/v2"
)

var (
	version = "dev"
	commit  = "none"    //nolint:unused // set via ldflags at build time
	date    = "unknown" //nolint:unused // set via ldflags at build time
)

// getPaths returns paths from positional args, defaulting to ["."]
func getPaths(c *cli.Context) []string {
	if c.Args().Len() > 0 {
		return c.Args().Slice()
	}
	return []string{"."}
}

func main() {
	app := &cli.App{
		Name:     "omen",
		Usage:    "Multi-language code analysis CLI",
		Version:  version,
		Metadata: make(map[string]interface{}),
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
			complexityCmd(),
			satdCmd(),
			deadcodeCmd(),
			churnCmd(),
			duplicatesCmd(),
			defectCmd(),
			tdgCmd(),
			graphCmd(),
			lintHotspotCmd(),
			contextCmd(),
			watchCmd(),
			analyzeCmd(),
		},
	}

	if err := app.Run(os.Args); err != nil {
		color.Red("Error: %v", err)
		os.Exit(1)
	}
}

func complexityCmd() *cli.Command {
	return &cli.Command{
		Name:      "complexity",
		Aliases:   []string{"cx"},
		Usage:     "Analyze cyclomatic and cognitive complexity",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
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
		},
		Action: runComplexityCmd,
	}
}

func runComplexityCmd(c *cli.Context) error {
	paths := getPaths(c)
	cycThreshold := c.Int("cyclomatic-threshold")
	cogThreshold := c.Int("cognitive-threshold")
	functionsOnly := c.Bool("functions-only")

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

	cxAnalyzer := analyzer.NewComplexityAnalyzer()
	defer cxAnalyzer.Close()

	tracker := progress.NewTracker("Analyzing complexity...", len(files))
	analysis, err := cxAnalyzer.AnalyzeProjectWithProgress(files, tracker.Tick)
	tracker.FinishSuccess()
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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
			fmt.Sprintf("Avg Cyc: %.1f", analysis.Summary.AvgCyclomatic),
			fmt.Sprintf("Avg Cog: %.1f", analysis.Summary.AvgCognitive),
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
	return &cli.Command{
		Name:      "satd",
		Aliases:   []string{"debt"},
		Usage:     "Detect self-admitted technical debt (TODO, FIXME, HACK)",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.StringSliceFlag{
				Name:  "patterns",
				Usage: "Additional patterns to detect",
			},
			&cli.BoolFlag{
				Name:  "include-test",
				Usage: "Include test files in analysis",
			},
		},
		Action: runSATDCmd,
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

	satdAnalyzer := analyzer.NewSATDAnalyzer()
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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
	return &cli.Command{
		Name:      "deadcode",
		Aliases:   []string{"dc"},
		Usage:     "Detect unused functions, variables, and unreachable code",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.Float64Flag{
				Name:  "confidence",
				Value: 0.8,
				Usage: "Minimum confidence threshold (0.0-1.0)",
			},
		},
		Action: runDeadCodeCmd,
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// For JSON/TOON, output raw analysis
	if formatter.Format() == output.FormatJSON || formatter.Format() == output.FormatTOON {
		return formatter.Output(analysis)
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
	return &cli.Command{
		Name:      "churn",
		Usage:     "Analyze git commit history for file churn",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.IntFlag{
				Name:  "days",
				Value: 90,
				Usage: "Number of days of history to analyze",
			},
			&cli.IntFlag{
				Name:  "top",
				Value: 20,
				Usage: "Show top N files by churn",
			},
		},
		Action: runChurnCmd,
	}
}

func runChurnCmd(c *cli.Context) error {
	paths := getPaths(c)
	days := c.Int("days")
	topN := c.Int("top")

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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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
			fmt.Sprintf("%d", fm.UniqueAuthors),
			fmt.Sprintf("+%d/-%d", fm.LinesAdded, fm.LinesDeleted),
			scoreStr,
		})
	}

	table := output.NewTable(
		fmt.Sprintf("File Churn (Last %d Days)", days),
		[]string{"File", "Commits", "Authors", "Lines Changed", "Churn Score"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", analysis.Summary.TotalFiles),
			fmt.Sprintf("Total Commits: %d", analysis.Summary.TotalCommits),
			fmt.Sprintf("Authors: %d", analysis.Summary.UniqueAuthors),
			"",
			fmt.Sprintf("Max: %.2f", analysis.Summary.MaxChurnScore),
		},
		analysis,
	)

	return formatter.Output(table)
}

func duplicatesCmd() *cli.Command {
	return &cli.Command{
		Name:      "duplicates",
		Aliases:   []string{"dup", "clones"},
		Usage:     "Detect code clones and duplicates",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
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
		},
		Action: runDuplicatesCmd,
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

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
	return &cli.Command{
		Name:      "defect",
		Aliases:   []string{"predict"},
		Usage:     "Predict defect probability using PMAT weights",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.BoolFlag{
				Name:  "high-risk-only",
				Usage: "Show only high-risk files",
			},
		},
		Action: runDefectCmd,
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Sort by probability (highest first)
	sort.Slice(analysis.Files, func(i, j int) bool {
		return analysis.Files[i].Probability > analysis.Files[j].Probability
	})

	var rows [][]string
	for _, ds := range analysis.Files {
		if highRiskOnly && ds.RiskLevel != models.RiskHigh && ds.RiskLevel != models.RiskCritical {
			continue
		}

		probStr := fmt.Sprintf("%.0f%%", ds.Probability*100)
		riskStr := string(ds.RiskLevel)
		switch ds.RiskLevel {
		case models.RiskCritical:
			probStr = color.RedString(probStr)
			riskStr = color.RedString(riskStr)
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
	return &cli.Command{
		Name:      "tdg",
		Usage:     "Calculate Technical Debt Gradient scores",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.IntFlag{
				Name:  "hotspots",
				Value: 10,
				Usage: "Number of hotspots to show",
			},
		},
		Action: runTDGCmd,
	}
}

func runTDGCmd(c *cli.Context) error {
	paths := getPaths(c)
	hotspots := c.Int("hotspots")

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

	tdgAnalyzer := analyzer.NewTDGAnalyzer(cfg.Analysis.ChurnDays)
	defer tdgAnalyzer.Close()

	analysis, err := tdgAnalyzer.AnalyzeProject(repoPath, files)
	if err != nil {
		return fmt.Errorf("analysis failed: %w", err)
	}

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	// Show top hotspots
	filesToShow := analysis.Files
	if len(filesToShow) > hotspots {
		filesToShow = filesToShow[:hotspots]
	}

	var rows [][]string
	for _, ts := range filesToShow {
		scoreStr := fmt.Sprintf("%.2f", ts.Value)
		sevStr := string(ts.Severity)
		switch ts.Severity {
		case models.TDGHighRisk:
			scoreStr = color.RedString(scoreStr)
			sevStr = color.RedString(sevStr)
		case models.TDGModerate:
			scoreStr = color.YellowString(scoreStr)
			sevStr = color.YellowString(sevStr)
		case models.TDGGood:
			scoreStr = color.CyanString(scoreStr)
			sevStr = color.CyanString(sevStr)
		case models.TDGExcellent:
			scoreStr = color.GreenString(scoreStr)
			sevStr = color.GreenString(sevStr)
		}

		rows = append(rows, []string{
			ts.FilePath,
			scoreStr,
			sevStr,
			fmt.Sprintf("%.2f", ts.Components.Complexity),
			fmt.Sprintf("%.2f", ts.Components.Churn),
			fmt.Sprintf("%.2f", ts.Components.Duplication),
		})
	}

	table := output.NewTable(
		fmt.Sprintf("Technical Debt Gradient (Top %d Hotspots)", hotspots),
		[]string{"File", "TDG Score", "Severity", "Complexity", "Churn", "Duplication"},
		rows,
		[]string{
			fmt.Sprintf("Total Files: %d", analysis.Summary.TotalFiles),
			fmt.Sprintf("Max Score: %.2f", analysis.Summary.MaxScore),
			fmt.Sprintf("Avg Score: %.2f", analysis.Summary.AvgScore),
			"", "", "",
		},
		analysis,
	)

	return formatter.Output(table)
}

func graphCmd() *cli.Command {
	return &cli.Command{
		Name:      "graph",
		Aliases:   []string{"dag"},
		Usage:     "Generate dependency graph (Mermaid output)",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.StringFlag{
				Name:  "scope",
				Value: "module",
				Usage: "Scope: file, function, module, package",
			},
			&cli.BoolFlag{
				Name:  "metrics",
				Usage: "Include PageRank and centrality metrics",
			},
		},
		Action: runGraphCmd,
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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
	return &cli.Command{
		Name:      "lint-hotspot",
		Aliases:   []string{"lh"},
		Usage:     "Identify files with high lint violation density",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.IntFlag{
				Name:  "top",
				Value: 10,
				Usage: "Show top N files",
			},
		},
		Action: runLintHotspotCmd,
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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

func contextCmd() *cli.Command {
	return &cli.Command{
		Name:      "context",
		Aliases:   []string{"ctx"},
		Usage:     "Generate deep context for LLM consumption",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.BoolFlag{
				Name:  "include-metrics",
				Usage: "Include complexity and quality metrics",
			},
			&cli.BoolFlag{
				Name:  "include-graph",
				Usage: "Include dependency graph",
			},
		},
		Action: runContextCmd,
	}
}

func runContextCmd(c *cli.Context) error {
	paths := getPaths(c)
	includeMetrics := c.Bool("include-metrics")
	includeGraph := c.Bool("include-graph")

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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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
			fmt.Printf("- **Avg Cyclomatic**: %.1f\n", analysis.Summary.AvgCyclomatic)
			fmt.Printf("- **Avg Cognitive**: %.1f\n", analysis.Summary.AvgCognitive)
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

	return nil
}

func watchCmd() *cli.Command {
	return &cli.Command{
		Name:      "watch",
		Usage:     "Watch for file changes and re-analyze",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.StringSliceFlag{
				Name:  "analyzers",
				Value: cli.NewStringSlice("complexity", "satd"),
				Usage: "Analyzers to run on change",
			},
			&cli.DurationFlag{
				Name:  "debounce",
				Value: 500,
				Usage: "Debounce duration in milliseconds",
			},
		},
		Action: runWatchCmd,
	}
}

func runWatchCmd(c *cli.Context) error {
	paths := getPaths(c)
	analyzers := c.StringSlice("analyzers")
	debounce := c.Duration("debounce")

	cfg := config.LoadOrDefault()

	absPath, err := filepath.Abs(paths[0])
	if err != nil {
		return fmt.Errorf("invalid path: %w", err)
	}

	watcher, err := watch.NewWatcher(absPath, cfg, debounce, analyzers)
	if err != nil {
		return fmt.Errorf("failed to create watcher: %w", err)
	}
	defer watcher.Stop()

	// Set up analyzer callback
	watcher.SetCallback(func(changedPath string) {
		for _, name := range analyzers {
			switch name {
			case "complexity":
				cxAnalyzer := analyzer.NewComplexityAnalyzer()
				fc, err := cxAnalyzer.AnalyzeFile(changedPath)
				cxAnalyzer.Close()
				if err != nil {
					color.Red("Complexity error: %v", err)
					continue
				}
				fmt.Printf("Complexity: %d functions, avg cyc: %.1f, avg cog: %.1f\n",
					len(fc.Functions), fc.AvgCyclomatic, fc.AvgCognitive)

			case "satd":
				satdAnalyzer := analyzer.NewSATDAnalyzer()
				debts, err := satdAnalyzer.AnalyzeFile(changedPath)
				if err != nil {
					color.Red("SATD error: %v", err)
					continue
				}
				if len(debts) > 0 {
					color.Yellow("SATD: %d debt markers found", len(debts))
					for _, d := range debts {
						fmt.Printf("  Line %d: %s - %s\n", d.Line, d.Marker, truncate(d.Description, 50))
					}
				} else {
					color.Green("SATD: No debt markers found")
				}

			default:
				color.Yellow("Unknown analyzer: %s", name)
			}
		}
	})

	// Handle Ctrl+C
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		<-sigChan
		fmt.Println("\nStopping watch...")
		cancel()
	}()

	return watcher.Start(ctx)
}

func analyzeCmd() *cli.Command {
	return &cli.Command{
		Name:      "analyze",
		Aliases:   []string{"all"},
		Usage:     "Run all analyzers and generate comprehensive report",
		ArgsUsage: "[path...]",
		Flags: []cli.Flag{
			&cli.StringSliceFlag{
				Name:  "exclude",
				Usage: "Analyzers to exclude",
			},
		},
		Action: runAnalyzeCmd,
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

	formatter, err := output.NewFormatter(output.ParseFormat(c.String("format")), c.String("output"), true)
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
		TDG        *models.TDGAnalysis        `json:"tdg,omitempty"`
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

	// 7. TDG (composite - uses sub-analyzers)
	if !excludeSet["tdg"] {
		tracker := progress.NewTracker("Calculating TDG scores...", 1)
		tdgAnalyzer := analyzer.NewTDGAnalyzer(cfg.Analysis.ChurnDays)
		results.TDG, _ = tdgAnalyzer.AnalyzeProject(repoPath, files)
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
		fmt.Fprintf(w, "  Avg Cyclomatic: %.1f, Avg Cognitive: %.1f\n", results.Complexity.Summary.AvgCyclomatic, results.Complexity.Summary.AvgCognitive)
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
			results.Churn.Summary.TotalFiles,
			results.Churn.Summary.TotalCommits,
			results.Churn.Summary.UniqueAuthors)
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
		fmt.Fprintf(w, "  Max Score: %.2f, Avg Score: %.2f\n",
			results.TDG.Summary.MaxScore, results.TDG.Summary.AvgScore)
		if len(results.TDG.Summary.Hotspots) > 0 {
			fmt.Fprintf(w, "  Top Hotspot: %s (%.2f)\n",
				results.TDG.Summary.Hotspots[0].FilePath, results.TDG.Summary.Hotspots[0].Value)
		}
	}

	return nil
}
