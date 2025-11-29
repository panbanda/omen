package main

import (
	"context"

	mcpserver "github.com/panbanda/omen/internal/mcp"
	"github.com/urfave/cli/v2"
)

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
		Action: runMCPCmd,
	}
}

func runMCPCmd(c *cli.Context) error {
	server := mcpserver.NewServer(version)
	return server.Run(context.Background())
}
