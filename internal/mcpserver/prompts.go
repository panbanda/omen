package mcpserver

import (
	"bytes"
	"context"
	"embed"
	"path/filepath"
	"strings"
	"text/template"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"gopkg.in/yaml.v3"
)

//go:embed prompts/*.md
var promptFiles embed.FS

// promptArgument defines a customization parameter for a prompt.
type promptArgument struct {
	Name        string `yaml:"name"`
	Description string `yaml:"description"`
	Required    bool   `yaml:"required"`
	Default     string `yaml:"default"`
}

// promptFrontmatter is parsed from YAML frontmatter in prompt files.
type promptFrontmatter struct {
	Name        string           `yaml:"name"`
	Title       string           `yaml:"title"`
	Description string           `yaml:"description"`
	Arguments   []promptArgument `yaml:"arguments"`
}

// registerPrompts discovers and registers all prompts from embedded markdown files.
func (s *Server) registerPrompts() {
	entries, err := promptFiles.ReadDir("prompts")
	if err != nil {
		return
	}

	for _, entry := range entries {
		if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".md") {
			continue
		}

		fallbackName := strings.TrimSuffix(entry.Name(), ".md")
		path := filepath.Join("prompts", entry.Name())

		content, err := promptFiles.ReadFile(path)
		if err != nil {
			continue
		}

		fm, body := parseFrontmatter(content)

		// Use frontmatter name if provided, otherwise fall back to filename
		name := fm.Name
		if name == "" {
			name = fallbackName
		}

		// Build MCP arguments from frontmatter
		var args []*mcp.PromptArgument
		for _, a := range fm.Arguments {
			args = append(args, &mcp.PromptArgument{
				Name:        a.Name,
				Description: a.Description,
				Required:    a.Required,
			})
		}

		prompt := &mcp.Prompt{
			Name:        name,
			Description: fm.Description,
			Arguments:   args,
		}
		s.server.AddPrompt(prompt, makePromptHandler(fm, body))
	}
}

// parseFrontmatter extracts YAML frontmatter and returns the parsed struct and body.
func parseFrontmatter(content []byte) (fm promptFrontmatter, body string) {
	if !bytes.HasPrefix(content, []byte("---\n")) {
		return promptFrontmatter{}, string(content)
	}

	rest := content[4:]
	end := bytes.Index(rest, []byte("\n---\n"))
	if end == -1 {
		return promptFrontmatter{}, string(content)
	}

	if err := yaml.Unmarshal(rest[:end], &fm); err != nil {
		return promptFrontmatter{}, string(content)
	}

	body = strings.TrimPrefix(string(rest[end+5:]), "\n")
	return fm, body
}

// templateFuncs provides helper functions for prompt templates.
var templateFuncs = template.FuncMap{
	"default": func(defaultVal, val string) string {
		if val == "" {
			return defaultVal
		}
		return val
	},
	"join":     strings.Join,
	"split":    strings.Split,
	"lower":    strings.ToLower,
	"upper":    strings.ToUpper,
	"contains": strings.Contains,
}

// extractDefaults builds a defaults map from frontmatter arguments.
func extractDefaults(args []promptArgument) map[string]string {
	defaults := make(map[string]string)
	for _, arg := range args {
		if arg.Default != "" {
			defaults[arg.Name] = arg.Default
		}
	}
	return defaults
}

// mergeArgs combines user arguments with defaults, user args take precedence.
func mergeArgs(userArgs map[string]string, defaults map[string]string) map[string]string {
	result := make(map[string]string)
	for k, v := range defaults {
		result[k] = v
	}
	for k, v := range userArgs {
		if v != "" {
			result[k] = v
		}
	}
	return result
}

func makePromptHandler(fm promptFrontmatter, body string) mcp.PromptHandler {
	// Parse template at registration time for early error detection
	tmpl, parseErr := template.New(fm.Name).Funcs(templateFuncs).Parse(body)

	// Extract defaults from frontmatter at registration time
	defaults := extractDefaults(fm.Arguments)

	return func(ctx context.Context, req *mcp.GetPromptRequest) (*mcp.GetPromptResult, error) {
		if parseErr != nil {
			// Fall back to raw body if template parsing failed
			return &mcp.GetPromptResult{
				Description: fm.Description,
				Messages: []*mcp.PromptMessage{
					{
						Role:    "user",
						Content: &mcp.TextContent{Text: body},
					},
				},
			}, nil
		}

		// Merge user arguments with defaults from frontmatter
		args := mergeArgs(req.Params.Arguments, defaults)

		// Execute template
		var buf bytes.Buffer
		if err := tmpl.Execute(&buf, args); err != nil {
			// Fall back to raw body on execution error
			return &mcp.GetPromptResult{
				Description: fm.Description,
				Messages: []*mcp.PromptMessage{
					{
						Role:    "user",
						Content: &mcp.TextContent{Text: body},
					},
				},
			}, nil
		}

		return &mcp.GetPromptResult{
			Description: fm.Description,
			Messages: []*mcp.PromptMessage{
				{
					Role:    "user",
					Content: &mcp.TextContent{Text: buf.String()},
				},
			},
		}, nil
	}
}
