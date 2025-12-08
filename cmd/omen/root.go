package main

import (
	"fmt"
	"os"
	"runtime"
	"runtime/pprof"

	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var (
	cfgFile      string
	verbose      bool
	pprofPrefix  string
	pprofCPUFile *os.File
)

var rootCmd = &cobra.Command{
	Use:   "omen",
	Short: "Multi-language code analysis CLI",
	Long: `Omen analyzes codebases for complexity, technical debt, dead code,
code duplication, defect prediction, and dependency graphs.

Supports: Go, Rust, Python, TypeScript, JavaScript, Java, C, C++, Ruby, PHP`,
	PersistentPreRunE: func(cmd *cobra.Command, args []string) error {
		if pprofPrefix != "" {
			f, err := os.Create(pprofPrefix + ".cpu.pprof")
			if err != nil {
				return fmt.Errorf("failed to create CPU profile: %w", err)
			}
			if err := pprof.StartCPUProfile(f); err != nil {
				f.Close()
				return fmt.Errorf("failed to start CPU profile: %w", err)
			}
			pprofCPUFile = f
		}
		return nil
	},
	PersistentPostRunE: func(cmd *cobra.Command, args []string) error {
		if pprofPrefix != "" {
			pprof.StopCPUProfile()
			if pprofCPUFile != nil {
				pprofCPUFile.Close()
				color.Green("CPU profile written to %s.cpu.pprof", pprofPrefix)
			}

			memFile, err := os.Create(pprofPrefix + ".mem.pprof")
			if err != nil {
				return fmt.Errorf("failed to create memory profile: %w", err)
			}
			defer memFile.Close()

			runtime.GC()
			if err := pprof.WriteHeapProfile(memFile); err != nil {
				return fmt.Errorf("failed to write memory profile: %w", err)
			}
			color.Green("Memory profile written to %s.mem.pprof", pprofPrefix)
		}
		return nil
	},
}

func init() {
	rootCmd.PersistentFlags().StringVarP(&cfgFile, "config", "c", "", "Path to config file (TOML, YAML, or JSON)")
	rootCmd.PersistentFlags().BoolVar(&verbose, "verbose", false, "Enable verbose output")
	rootCmd.PersistentFlags().StringVar(&pprofPrefix, "pprof", "", "Enable pprof profiling (creates <prefix>.cpu.pprof and <prefix>.mem.pprof)")
}
