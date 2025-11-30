package mcpserver

import (
	"context"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

// Server wraps the MCP server and registers all omen analysis tools.
type Server struct {
	server *mcp.Server
}

// NewServer creates a new MCP server with all omen tools registered.
func NewServer(version string) *Server {
	if version == "" {
		version = "dev"
	}
	server := mcp.NewServer(
		&mcp.Implementation{
			Name:    "omen",
			Version: version,
		},
		nil,
	)

	s := &Server{server: server}
	s.registerTools()
	s.registerPrompts()
	return s
}

// Run starts the MCP server over stdio transport.
func (s *Server) Run(ctx context.Context) error {
	return s.server.Run(ctx, &mcp.StdioTransport{})
}

// registerTools adds all omen analyzer tools to the server.
func (s *Server) registerTools() {
	// Complexity analysis
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_complexity",
		Description: describeComplexity(),
	}, handleAnalyzeComplexity)

	// SATD (Self-Admitted Technical Debt)
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_satd",
		Description: describeSATD(),
	}, handleAnalyzeSATD)

	// Dead code detection
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_deadcode",
		Description: describeDeadcode(),
	}, handleAnalyzeDeadcode)

	// Churn analysis (git)
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_churn",
		Description: describeChurn(),
	}, handleAnalyzeChurn)

	// Duplicate/clone detection
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_duplicates",
		Description: describeDuplicates(),
	}, handleAnalyzeDuplicates)

	// Defect prediction
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_defect",
		Description: describeDefect(),
	}, handleAnalyzeDefect)

	// Technical Debt Gradient
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_tdg",
		Description: describeTDG(),
	}, handleAnalyzeTDG)

	// Dependency graph
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_graph",
		Description: describeGraph(),
	}, handleAnalyzeGraph)

	// Hotspot analysis (churn + complexity)
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_hotspot",
		Description: describeHotspot(),
	}, handleAnalyzeHotspot)

	// Temporal coupling
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_temporal_coupling",
		Description: describeTemporalCoupling(),
	}, handleAnalyzeTemporalCoupling)

	// Ownership/bus factor
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_ownership",
		Description: describeOwnership(),
	}, handleAnalyzeOwnership)

	// Cohesion (CK metrics)
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_cohesion",
		Description: describeCohesion(),
	}, handleAnalyzeCohesion)

	// Repository map (PageRank symbols)
	mcp.AddTool(s.server, &mcp.Tool{
		Name:        "analyze_repo_map",
		Description: describeRepoMap(),
	}, handleAnalyzeRepoMap)
}
