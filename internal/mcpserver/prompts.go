package mcpserver

import (
	"context"
	"embed"
	"fmt"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

//go:embed prompts/*.md
var promptFiles embed.FS

// promptDefinitions lists all available prompts.
// Each prompt's content is embedded from prompts/*.md files.
var promptDefinitions = []struct {
	Name        string
	Description string
	ContentFile string
}{
	{
		Name:        "context-compression",
		Description: "Generate a compressed context summary of a codebase for LLM consumption, using PageRank-ranked symbols and dependency graphs.",
		ContentFile: "prompts/context-compression.md",
	},
	{
		Name:        "refactoring-priority",
		Description: "Identify highest-priority refactoring targets based on TDG scores, complexity, code clones, and technical debt markers.",
		ContentFile: "prompts/refactoring-priority.md",
	},
	{
		Name:        "bug-hunt",
		Description: "Find the most likely locations for bugs using defect prediction, hotspot analysis, temporal coupling, and ownership patterns.",
		ContentFile: "prompts/bug-hunt.md",
	},
	{
		Name:        "change-impact",
		Description: "Analyze the potential impact of changes to specific files, including dependencies, temporal coupling, and ownership.",
		ContentFile: "prompts/change-impact.md",
	},
	{
		Name:        "codebase-onboarding",
		Description: "Generate an onboarding guide for developers new to a codebase, including key symbols, architecture, and subject matter experts.",
		ContentFile: "prompts/codebase-onboarding.md",
	},
	{
		Name:        "code-review-focus",
		Description: "Identify what to focus on when reviewing code changes, including complexity deltas, duplication, and risk assessment.",
		ContentFile: "prompts/code-review-focus.md",
	},
	{
		Name:        "architecture-review",
		Description: "Analyze architectural health including module coupling, cohesion metrics, hidden dependencies, and design smells.",
		ContentFile: "prompts/architecture-review.md",
	},
	{
		Name:        "tech-debt-report",
		Description: "Generate a comprehensive technical debt assessment including SATD, quality grades, duplication, and high-risk areas.",
		ContentFile: "prompts/tech-debt-report.md",
	},
	{
		Name:        "test-targeting",
		Description: "Identify which files and functions most need additional test coverage based on risk, complexity, and churn.",
		ContentFile: "prompts/test-targeting.md",
	},
	{
		Name:        "quality-gate",
		Description: "Perform a quality gate check against configurable thresholds for TDG grade, complexity, duplication, and defect risk.",
		ContentFile: "prompts/quality-gate.md",
	},
}

// registerPrompts adds all prompts to the MCP server.
func (s *Server) registerPrompts() {
	for _, def := range promptDefinitions {
		d := def // capture for closure
		prompt := &mcp.Prompt{
			Name:        d.Name,
			Description: d.Description,
		}
		s.server.AddPrompt(prompt, func(ctx context.Context, req *mcp.GetPromptRequest) (*mcp.GetPromptResult, error) {
			content, err := promptFiles.ReadFile(d.ContentFile)
			if err != nil {
				return nil, fmt.Errorf("failed to read prompt: %w", err)
			}
			return &mcp.GetPromptResult{
				Description: d.Description,
				Messages: []*mcp.PromptMessage{
					{
						Role:    "user",
						Content: &mcp.TextContent{Text: string(content)},
					},
				},
			}, nil
		})
	}
}
