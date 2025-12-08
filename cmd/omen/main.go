package main

import (
	"os"
)

var (
	version = "dev"
	commit  = "none"    //nolint:unused // set via ldflags at build time
	date    = "unknown" //nolint:unused // set via ldflags at build time
)

func init() {
	rootCmd.Version = version
}

func main() {
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
