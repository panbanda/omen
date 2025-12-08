package main

import (
	"fmt"

	"github.com/fatih/color"
	"github.com/panbanda/omen/pkg/config"
	"github.com/pelletier/go-toml"
	"github.com/spf13/cobra"
)

var configCmd = &cobra.Command{
	Use:   "config",
	Short: "Configuration management commands",
}

var configValidateCmd = &cobra.Command{
	Use:   "validate",
	Short: "Validate a configuration file",
	Long: `Validates an omen configuration file for syntax errors and invalid values.

Examples:
  omen config validate                  # Validates default config locations
  omen config validate -c omen.toml     # Validates specific file
  omen config validate -c .omen/omen.toml`,
	RunE: runConfigValidate,
}

var configShowCmd = &cobra.Command{
	Use:   "show",
	Short: "Show the effective configuration",
	Long: `Shows the merged configuration from defaults and config file.

Examples:
  omen config show              # Show effective config
  omen config show -c omen.toml # Show config from specific file`,
	RunE: runConfigShow,
}

func init() {
	configValidateCmd.Flags().StringP("config", "c", "", "Path to config file to validate")
	configShowCmd.Flags().StringP("config", "c", "", "Path to config file")

	configCmd.AddCommand(configValidateCmd)
	configCmd.AddCommand(configShowCmd)
	rootCmd.AddCommand(configCmd)
}

func runConfigValidate(cmd *cobra.Command, args []string) error {
	var opts []config.LoadOption
	if path, _ := cmd.Flags().GetString("config"); path != "" {
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

func runConfigShow(cmd *cobra.Command, args []string) error {
	var opts []config.LoadOption
	if path, _ := cmd.Flags().GetString("config"); path != "" {
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
