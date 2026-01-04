package main

import (
	"context"
	"fmt"
	"path/filepath"
	"time"

	"github.com/fatih/color"
	"github.com/panbanda/omen/internal/output"
	"github.com/panbanda/omen/internal/progress"
	"github.com/panbanda/omen/internal/service/analysis"
	scannerSvc "github.com/panbanda/omen/internal/service/scanner"
	"github.com/panbanda/omen/pkg/analyzer/churn"
	"github.com/panbanda/omen/pkg/analyzer/cohesion"
	"github.com/panbanda/omen/pkg/analyzer/complexity"
	"github.com/panbanda/omen/pkg/analyzer/deadcode"
	"github.com/panbanda/omen/pkg/analyzer/defect"
	"github.com/panbanda/omen/pkg/analyzer/duplicates"
	"github.com/panbanda/omen/pkg/analyzer/featureflags"
	"github.com/panbanda/omen/pkg/analyzer/hotspot"
	"github.com/panbanda/omen/pkg/analyzer/ownership"
	"github.com/panbanda/omen/pkg/analyzer/satd"
	"github.com/panbanda/omen/pkg/analyzer/smells"
	"github.com/panbanda/omen/pkg/analyzer/tdg"
	"github.com/panbanda/omen/pkg/analyzer/temporal"
	"github.com/spf13/cobra"
)

var analyzeCmd = &cobra.Command{
	Use:     "analyze [path...]",
	Aliases: []string{"a"},
	Short:   "Run code analysis (all analyzers if no subcommand specified)",
	RunE:    runAnalyze,
}

// fullAnalysis holds comprehensive analysis results from all analyzers.
type fullAnalysis struct {
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

func init() {
	// Persistent flags inherited by all analyzer subcommands
	analyzeCmd.PersistentFlags().StringP("format", "f", "text", "Output format: text, json, markdown")
	analyzeCmd.PersistentFlags().StringP("output", "o", "", "Write output to file")
	analyzeCmd.PersistentFlags().Bool("no-cache", false, "Disable caching")
	analyzeCmd.PersistentFlags().String("ref", "", "Git ref (branch, tag, SHA) for remote repositories")
	analyzeCmd.PersistentFlags().Bool("shallow", false, "Shallow clone (depth=1) for remote repos; disables git history analyzers")

	// Local flags for analyze command itself
	analyzeCmd.Flags().StringSlice("exclude", nil, "Analyzers to exclude (when running all)")

	rootCmd.AddCommand(analyzeCmd)
}

// getFormat returns the format flag value from the command.
func getFormat(cmd *cobra.Command) string {
	format, _ := cmd.Flags().GetString("format")
	return format
}

// getOutputFile returns the output file path from the command.
func getOutputFile(cmd *cobra.Command) string {
	outputFile, _ := cmd.Flags().GetString("output")
	return outputFile
}

// getSort returns the sort flag value from the command.
func getSort(cmd *cobra.Command, defaultValue string) string {
	sort, _ := cmd.Flags().GetString("sort")
	if sort == "" {
		return defaultValue
	}
	return sort
}

func runAnalyze(cmd *cobra.Command, args []string) error {
	ref, _ := cmd.Flags().GetString("ref")
	shallow, _ := cmd.Flags().GetBool("shallow")

	paths, cleanup, err := resolvePaths(cmd.Context(), args, ref, shallow)
	if err != nil {
		return err
	}
	defer cleanup()

	exclude, _ := cmd.Flags().GetStringSlice("exclude")

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

	formatter, err := output.NewFormatter(output.ParseFormat(getFormat(cmd)), getOutputFile(cmd), true)
	if err != nil {
		return err
	}
	defer formatter.Close()

	excludeSet := make(map[string]bool)
	for _, e := range exclude {
		excludeSet[e] = true
	}

	results := fullAnalysis{}

	startTime := time.Now()
	color.Cyan("Running comprehensive analysis on %d files...\n", len(files))

	svc := analysis.New()

	// Run all analyzers
	if !excludeSet["complexity"] {
		tracker := progress.NewTracker("Analyzing complexity...", len(files))
		results.Complexity, _ = svc.AnalyzeComplexity(context.Background(), files, analysis.ComplexityOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	if !excludeSet["satd"] {
		tracker := progress.NewTracker("Detecting technical debt...", len(files))
		results.SATD, _ = svc.AnalyzeSATD(context.Background(), files, analysis.SATDOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	if !excludeSet["deadcode"] {
		tracker := progress.NewTracker("Detecting dead code...", len(files))
		results.DeadCode, _ = svc.AnalyzeDeadCode(context.Background(), files, analysis.DeadCodeOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

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

	if !excludeSet["duplicates"] {
		tracker := progress.NewTracker("Detecting duplicates...", len(files))
		results.Clones, _ = svc.AnalyzeDuplicates(context.Background(), files, analysis.DuplicatesOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	if !excludeSet["defect"] {
		tracker := progress.NewTracker("Predicting defects...", 1)
		results.Defect, _ = svc.AnalyzeDefects(context.Background(), repoPath, files, analysis.DefectOptions{})
		tracker.FinishSuccess()
	}

	if !excludeSet["tdg"] {
		tracker := progress.NewTracker("Calculating TDG scores...", len(files))
		results.TDG, _ = svc.AnalyzeTDG(context.Background(), files)
		tracker.FinishSuccess()
	}

	if !excludeSet["hotspots"] {
		spinner := progress.NewSpinner("Analyzing hotspots...")
		results.Hotspots, _ = svc.AnalyzeHotspots(context.Background(), repoPath, files, analysis.HotspotOptions{})
		if results.Hotspots != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	if !excludeSet["smells"] {
		tracker := progress.NewTracker("Detecting architectural smells...", len(files))
		results.Smells, _ = svc.AnalyzeSmells(context.Background(), files, analysis.SmellOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	if !excludeSet["ownership"] {
		spinner := progress.NewSpinner("Analyzing code ownership...")
		results.Ownership, _ = svc.AnalyzeOwnership(context.Background(), repoPath, files, analysis.OwnershipOptions{})
		if results.Ownership != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	if !excludeSet["temporal-coupling"] {
		spinner := progress.NewSpinner("Analyzing temporal coupling...")
		results.TemporalCoupling, _ = svc.AnalyzeTemporalCoupling(context.Background(), repoPath, analysis.TemporalCouplingOptions{})
		if results.TemporalCoupling != nil {
			spinner.FinishSuccess()
		} else {
			spinner.FinishSkipped("not a git repo")
		}
	}

	if !excludeSet["cohesion"] {
		tracker := progress.NewTracker("Analyzing cohesion metrics...", len(files))
		results.Cohesion, _ = svc.AnalyzeCohesion(context.Background(), files, analysis.CohesionOptions{
			OnProgress: tracker.Tick,
		})
		tracker.FinishSuccess()
	}

	if !excludeSet["flags"] {
		tracker := progress.NewTracker("Detecting feature flags...", len(files))
		results.FeatureFlags, _ = svc.AnalyzeFeatureFlags(context.Background(), files, analysis.FeatureFlagOptions{
			OnProgress: tracker.Tick,
			IncludeGit: true,
		})
		tracker.FinishSuccess()
	}

	elapsed := time.Since(startTime)
	fmt.Printf("\nAnalysis completed in %s\n\n", elapsed.Round(time.Millisecond))

	// For JSON, output raw results
	if formatter.Format() == output.FormatJSON {
		return formatter.Output(results)
	}

	// Print summary report (text/markdown)
	return printAnalysisSummary(formatter, results)
}

func printAnalysisSummary(formatter *output.Formatter, r fullAnalysis) error {
	w := formatter.Writer()

	if formatter.Colored() {
		color.Cyan("=== Analysis Summary ===\n")
	} else {
		fmt.Fprintln(w, "=== Analysis Summary ===")
	}

	if r.Complexity != nil {
		fmt.Fprintf(w, "\nComplexity:\n")
		fmt.Fprintf(w, "  Files: %d, Functions: %d\n", r.Complexity.Summary.TotalFiles, r.Complexity.Summary.TotalFunctions)
		fmt.Fprintf(w, "  Median Cyclomatic (P50): %d, Median Cognitive (P50): %d\n", r.Complexity.Summary.P50Cyclomatic, r.Complexity.Summary.P50Cognitive)
		fmt.Fprintf(w, "  90th Percentile Cyclomatic: %d, 90th Percentile Cognitive: %d\n", r.Complexity.Summary.P90Cyclomatic, r.Complexity.Summary.P90Cognitive)
		fmt.Fprintf(w, "  Max Cyclomatic: %d, Max Cognitive: %d\n", r.Complexity.Summary.MaxCyclomatic, r.Complexity.Summary.MaxCognitive)
	}

	if r.SATD != nil {
		fmt.Fprintf(w, "\nTechnical Debt:\n")
		fmt.Fprintf(w, "  Total: %d items (High: %d, Medium: %d, Low: %d)\n",
			r.SATD.Summary.TotalItems,
			r.SATD.Summary.BySeverity["high"],
			r.SATD.Summary.BySeverity["medium"],
			r.SATD.Summary.BySeverity["low"])
	}

	if r.DeadCode != nil {
		fmt.Fprintf(w, "\nDead Code:\n")
		fmt.Fprintf(w, "  Functions: %d, Variables: %d (%.1f%% dead)\n",
			r.DeadCode.Summary.TotalDeadFunctions,
			r.DeadCode.Summary.TotalDeadVariables,
			r.DeadCode.Summary.DeadCodePercentage)
	}

	if r.Churn != nil {
		fmt.Fprintf(w, "\nFile Churn:\n")
		fmt.Fprintf(w, "  Files: %d, File Changes: %d, Authors: %d\n",
			r.Churn.Summary.TotalFilesChanged,
			r.Churn.Summary.TotalFileChanges,
			len(r.Churn.Summary.AuthorContributions))
	}

	if r.Clones != nil {
		fmt.Fprintf(w, "\nCode Clones:\n")
		fmt.Fprintf(w, "  Total: %d (Type-1: %d, Type-2: %d, Type-3: %d)\n",
			r.Clones.Summary.TotalClones,
			r.Clones.Summary.Type1Count,
			r.Clones.Summary.Type2Count,
			r.Clones.Summary.Type3Count)
	}

	if r.Defect != nil {
		fmt.Fprintf(w, "\nDefect Prediction:\n")
		fmt.Fprintf(w, "  High Risk: %d, Medium Risk: %d, Low Risk: %d\n",
			r.Defect.Summary.HighRiskCount,
			r.Defect.Summary.MediumRiskCount,
			r.Defect.Summary.LowRiskCount)
		fmt.Fprintf(w, "  Avg Probability: %.0f%%\n", r.Defect.Summary.AvgProbability*100)
	}

	if r.TDG != nil {
		fmt.Fprintf(w, "\nTechnical Debt Gradient:\n")
		fmt.Fprintf(w, "  Files: %d, Avg Score: %.1f, Grade: %s\n",
			r.TDG.TotalFiles, r.TDG.AverageScore, r.TDG.AverageGrade)
		if len(r.TDG.Files) > 0 {
			worst := r.TDG.Files[0]
			for _, f := range r.TDG.Files {
				if f.Total < worst.Total {
					worst = f
				}
			}
			fmt.Fprintf(w, "  Lowest Score: %s (%.1f, %s)\n",
				worst.FilePath, worst.Total, worst.Grade)
		}
		if len(r.TDG.GradeDistribution) > 0 {
			fmt.Fprintf(w, "  Grade Distribution:\n")
			gradeOrder := []tdg.Grade{
				tdg.GradeAPlus, tdg.GradeA, tdg.GradeAMinus,
				tdg.GradeBPlus, tdg.GradeB, tdg.GradeBMinus,
				tdg.GradeCPlus, tdg.GradeC, tdg.GradeCMinus,
				tdg.GradeD, tdg.GradeF,
			}
			for _, grade := range gradeOrder {
				if count := r.TDG.GradeDistribution[grade]; count > 0 {
					percentage := float64(count) / float64(r.TDG.TotalFiles) * 100
					fmt.Fprintf(w, "    - %s: %d files (%.1f%%)\n", grade, count, percentage)
				}
			}
		}
	}

	if r.Hotspots != nil {
		fmt.Fprintf(w, "\nHotspots:\n")
		fmt.Fprintf(w, "  Files: %d, Hotspots (score >= 0.4): %d\n",
			r.Hotspots.Summary.TotalFiles,
			r.Hotspots.Summary.HotspotCount)
		fmt.Fprintf(w, "  P50 Score: %.2f, P90 Score: %.2f, Max: %.2f\n",
			r.Hotspots.Summary.P50HotspotScore,
			r.Hotspots.Summary.P90HotspotScore,
			r.Hotspots.Summary.MaxHotspotScore)
	}

	if r.Smells != nil {
		fmt.Fprintf(w, "\nArchitectural Smells:\n")
		fmt.Fprintf(w, "  Total: %d (Critical: %d, High: %d, Medium: %d)\n",
			r.Smells.Summary.TotalSmells,
			r.Smells.Summary.CriticalCount,
			r.Smells.Summary.HighCount,
			r.Smells.Summary.MediumCount)
		fmt.Fprintf(w, "  Cycles: %d, Hubs: %d, God Components: %d, Unstable: %d\n",
			r.Smells.Summary.CyclicCount,
			r.Smells.Summary.HubCount,
			r.Smells.Summary.GodCount,
			r.Smells.Summary.UnstableCount)
	}

	if r.Ownership != nil {
		fmt.Fprintf(w, "\nCode Ownership:\n")
		fmt.Fprintf(w, "  Files: %d, Bus Factor: %d, Silos: %d\n",
			r.Ownership.Summary.TotalFiles,
			r.Ownership.Summary.BusFactor,
			r.Ownership.Summary.SiloCount)
		fmt.Fprintf(w, "  Avg Contributors/File: %.1f\n", r.Ownership.Summary.AvgContributors)
	}

	if r.TemporalCoupling != nil {
		fmt.Fprintf(w, "\nTemporal Coupling:\n")
		fmt.Fprintf(w, "  Couplings: %d, Strong (>= 0.5): %d\n",
			r.TemporalCoupling.Summary.TotalCouplings,
			r.TemporalCoupling.Summary.StrongCouplings)
		fmt.Fprintf(w, "  Avg Strength: %.2f, Max: %.2f\n",
			r.TemporalCoupling.Summary.AvgCouplingStrength,
			r.TemporalCoupling.Summary.MaxCouplingStrength)
	}

	if r.Cohesion != nil {
		fmt.Fprintf(w, "\nCohesion (CK Metrics):\n")
		fmt.Fprintf(w, "  Classes: %d, Files: %d\n",
			r.Cohesion.Summary.TotalClasses,
			r.Cohesion.Summary.TotalFiles)
		fmt.Fprintf(w, "  Avg WMC: %.1f, Avg CBO: %.1f, Avg LCOM: %.1f\n",
			r.Cohesion.Summary.AvgWMC,
			r.Cohesion.Summary.AvgCBO,
			r.Cohesion.Summary.AvgLCOM)
		fmt.Fprintf(w, "  Low Cohesion (LCOM > 1): %d classes\n",
			r.Cohesion.Summary.LowCohesionCount)
	}

	if r.FeatureFlags != nil {
		fmt.Fprintf(w, "\nFeature Flags:\n")
		fmt.Fprintf(w, "  Flags: %d, References: %d\n",
			r.FeatureFlags.Summary.TotalFlags,
			r.FeatureFlags.Summary.TotalReferences)
		fmt.Fprintf(w, "  By Priority: Critical: %d, High: %d, Medium: %d, Low: %d\n",
			r.FeatureFlags.Summary.ByPriority["CRITICAL"],
			r.FeatureFlags.Summary.ByPriority["HIGH"],
			r.FeatureFlags.Summary.ByPriority["MEDIUM"],
			r.FeatureFlags.Summary.ByPriority["LOW"])
		fmt.Fprintf(w, "  Avg File Spread: %.1f, Max: %d\n",
			r.FeatureFlags.Summary.AvgFileSpread,
			r.FeatureFlags.Summary.MaxFileSpread)
	}

	return nil
}
