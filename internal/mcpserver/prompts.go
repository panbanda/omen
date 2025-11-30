package mcpserver

import (
	"context"
	"embed"
	"fmt"
	"strconv"
	"strings"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

//go:embed prompts/*.md
var promptFiles embed.FS

// PromptDefinition describes a prompt with its metadata and content.
type PromptDefinition struct {
	Name        string
	Description string
	Arguments   []*mcp.PromptArgument
	ContentFile string
}

// promptDefinitions lists all available prompts.
var promptDefinitions = []PromptDefinition{
	{
		Name:        "context-compression",
		Description: "Generate a compressed context summary of a codebase for LLM consumption, using PageRank-ranked symbols and dependency graphs.",
		Arguments: []*mcp.PromptArgument{
			{Name: "top", Description: "Number of top symbols to include (default: 30)", Required: false},
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/context-compression.md",
	},
	{
		Name:        "refactoring-priority",
		Description: "Identify highest-priority refactoring targets based on TDG scores, complexity, code clones, and technical debt markers.",
		Arguments: []*mcp.PromptArgument{
			{Name: "count", Description: "Number of hotspots to analyze (default: 10)", Required: false},
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/refactoring-priority.md",
	},
	{
		Name:        "bug-hunt",
		Description: "Find the most likely locations for bugs using defect prediction, hotspot analysis, temporal coupling, and ownership patterns.",
		Arguments: []*mcp.PromptArgument{
			{Name: "days", Description: "Days of git history to analyze (default: 30)", Required: false},
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/bug-hunt.md",
	},
	{
		Name:        "change-impact",
		Description: "Analyze the potential impact of changes to specific files, including dependencies, temporal coupling, and ownership.",
		Arguments: []*mcp.PromptArgument{
			{Name: "target", Description: "File or function to analyze impact for", Required: true},
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/change-impact.md",
	},
	{
		Name:        "codebase-onboarding",
		Description: "Generate an onboarding guide for developers new to a codebase, including key symbols, architecture, and subject matter experts.",
		Arguments: []*mcp.PromptArgument{
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/codebase-onboarding.md",
	},
	{
		Name:        "code-review-focus",
		Description: "Identify what to focus on when reviewing code changes, including complexity deltas, duplication, and risk assessment.",
		Arguments: []*mcp.PromptArgument{
			{Name: "files", Description: "Changed files to analyze (comma-separated)", Required: true},
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/code-review-focus.md",
	},
	{
		Name:        "architecture-review",
		Description: "Analyze architectural health including module coupling, cohesion metrics, hidden dependencies, and design smells.",
		Arguments: []*mcp.PromptArgument{
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/architecture-review.md",
	},
	{
		Name:        "tech-debt-report",
		Description: "Generate a comprehensive technical debt assessment including SATD, quality grades, duplication, and high-risk areas.",
		Arguments: []*mcp.PromptArgument{
			{Name: "count", Description: "Number of hotspots to include (default: 20)", Required: false},
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/tech-debt-report.md",
	},
	{
		Name:        "test-targeting",
		Description: "Identify which files and functions most need additional test coverage based on risk, complexity, and churn.",
		Arguments: []*mcp.PromptArgument{
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
		ContentFile: "prompts/test-targeting.md",
	},
	{
		Name:        "quality-gate",
		Description: "Perform a quality gate check against configurable thresholds for TDG grade, complexity, duplication, and defect risk.",
		Arguments: []*mcp.PromptArgument{
			{Name: "paths", Description: "Paths to analyze (default: current directory)", Required: false},
		},
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
			Arguments:   d.Arguments,
		}
		s.server.AddPrompt(prompt, makePromptHandler(d))
	}
}

// makePromptHandler creates a handler function for a prompt definition.
func makePromptHandler(def PromptDefinition) func(context.Context, *mcp.GetPromptRequest) (*mcp.GetPromptResult, error) {
	return func(ctx context.Context, req *mcp.GetPromptRequest) (*mcp.GetPromptResult, error) {
		// Read the template content
		content, err := promptFiles.ReadFile(def.ContentFile)
		if err != nil {
			return nil, fmt.Errorf("failed to read prompt template: %w", err)
		}

		// Apply argument substitutions
		text := string(content)
		args := req.Params.Arguments

		// Apply default values and substitutions
		text = substituteArg(text, "top", args, "30")
		text = substituteArg(text, "count", args, "10")
		text = substituteArg(text, "days", args, "30")
		text = substituteArg(text, "paths", args, ".")
		text = substituteArg(text, "target", args, "")
		text = substituteArg(text, "files", args, "")

		// Build tool call suggestions based on the prompt
		toolCalls := buildToolCallSuggestions(def.Name, args)

		// Append tool call suggestions to the prompt
		if len(toolCalls) > 0 {
			text += "\n\n## Suggested Tool Calls\n\n"
			text += "Execute these omen tools to gather the data needed:\n\n"
			for _, tc := range toolCalls {
				text += fmt.Sprintf("- `%s`\n", tc)
			}
		}

		return &mcp.GetPromptResult{
			Description: def.Description,
			Messages: []*mcp.PromptMessage{
				{
					Role:    "user",
					Content: &mcp.TextContent{Text: text},
				},
			},
		}, nil
	}
}

// substituteArg replaces {{key}} with the argument value or default.
func substituteArg(text, key string, args map[string]string, defaultVal string) string {
	val := defaultVal
	if v, ok := args[key]; ok && v != "" {
		val = v
	}
	return strings.ReplaceAll(text, "{{"+key+"}}", val)
}

// buildToolCallSuggestions generates suggested tool calls based on the prompt type.
func buildToolCallSuggestions(promptName string, args map[string]string) []string {
	paths := "."
	if p, ok := args["paths"]; ok && p != "" {
		paths = p
	}

	top := 30
	if t, ok := args["top"]; ok && t != "" {
		if n, err := strconv.Atoi(t); err == nil {
			top = n
		}
	}

	count := 10
	if c, ok := args["count"]; ok && c != "" {
		if n, err := strconv.Atoi(c); err == nil {
			count = n
		}
	}

	days := 30
	if d, ok := args["days"]; ok && d != "" {
		if n, err := strconv.Atoi(d); err == nil {
			days = n
		}
	}

	switch promptName {
	case "context-compression":
		return []string{
			fmt.Sprintf(`analyze_repo_map(paths: ["%s"], top: %d)`, paths, top),
			fmt.Sprintf(`analyze_graph(paths: ["%s"], scope: "module", include_metrics: true)`, paths),
		}

	case "refactoring-priority":
		return []string{
			fmt.Sprintf(`analyze_tdg(paths: ["%s"], hotspots: %d)`, paths, count),
			fmt.Sprintf(`analyze_complexity(paths: ["%s"], functions_only: true)`, paths),
			fmt.Sprintf(`analyze_duplicates(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_satd(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_cohesion(paths: ["%s"])`, paths),
		}

	case "bug-hunt":
		return []string{
			fmt.Sprintf(`analyze_defect(paths: ["%s"], high_risk_only: true)`, paths),
			fmt.Sprintf(`analyze_hotspot(paths: ["%s"], days: %d)`, paths, days),
			fmt.Sprintf(`analyze_temporal_coupling(paths: ["%s"], days: %d)`, paths, days),
			fmt.Sprintf(`analyze_ownership(paths: ["%s"])`, paths),
		}

	case "change-impact":
		target := args["target"]
		return []string{
			fmt.Sprintf(`analyze_graph(paths: ["%s"], scope: "function", include_metrics: true)`, paths),
			fmt.Sprintf(`analyze_temporal_coupling(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_ownership(paths: ["%s"])`, paths),
			fmt.Sprintf("# Focus analysis on: %s", target),
		}

	case "codebase-onboarding":
		return []string{
			fmt.Sprintf(`analyze_repo_map(paths: ["%s"], top: 30)`, paths),
			fmt.Sprintf(`analyze_graph(paths: ["%s"], scope: "module")`, paths),
			fmt.Sprintf(`analyze_ownership(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_complexity(paths: ["%s"])`, paths),
		}

	case "code-review-focus":
		files := args["files"]
		return []string{
			fmt.Sprintf(`analyze_complexity(paths: ["%s"])`, files),
			fmt.Sprintf(`analyze_duplicates(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_deadcode(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_defect(paths: ["%s"])`, paths),
		}

	case "architecture-review":
		return []string{
			fmt.Sprintf(`analyze_graph(paths: ["%s"], scope: "module", include_metrics: true)`, paths),
			fmt.Sprintf(`analyze_cohesion(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_temporal_coupling(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_ownership(paths: ["%s"])`, paths),
		}

	case "tech-debt-report":
		return []string{
			fmt.Sprintf(`analyze_satd(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_tdg(paths: ["%s"], hotspots: %d)`, paths, count),
			fmt.Sprintf(`analyze_duplicates(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_defect(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_complexity(paths: ["%s"])`, paths),
		}

	case "test-targeting":
		return []string{
			fmt.Sprintf(`analyze_defect(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_hotspot(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_complexity(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_ownership(paths: ["%s"])`, paths),
		}

	case "quality-gate":
		return []string{
			fmt.Sprintf(`analyze_tdg(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_complexity(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_duplicates(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_defect(paths: ["%s"])`, paths),
			fmt.Sprintf(`analyze_satd(paths: ["%s"])`, paths),
		}

	default:
		return nil
	}
}
