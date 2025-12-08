package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/fatih/color"
	"github.com/panbanda/omen/pkg/config"
	"github.com/pelletier/go-toml"
	"github.com/spf13/cobra"
)

var initCmd = &cobra.Command{
	Use:   "init",
	Short: "Initialize a new omen configuration file",
	Long: `Creates a new omen.toml configuration file in the current directory
with sensible defaults. Use --output to specify a different location.

Examples:
  omen init                    # Creates omen.toml in current directory
  omen init -o .omen/omen.toml # Creates config in .omen directory
  omen init --force            # Overwrite existing config file`,
	RunE: runInit,
}

func init() {
	initCmd.Flags().StringP("output", "o", "omen.toml", "Output file path")
	initCmd.Flags().Bool("force", false, "Overwrite existing config file")

	rootCmd.AddCommand(initCmd)
}

func runInit(cmd *cobra.Command, args []string) error {
	outputPath, _ := cmd.Flags().GetString("output")
	force, _ := cmd.Flags().GetBool("force")

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
