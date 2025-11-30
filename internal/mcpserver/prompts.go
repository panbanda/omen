package mcpserver

import (
	"bytes"
	"context"
	"embed"
	"path/filepath"
	"strings"

	"github.com/modelcontextprotocol/go-sdk/mcp"
	"gopkg.in/yaml.v3"
)

//go:embed prompts/*.md
var promptFiles embed.FS

// promptFrontmatter is parsed from YAML frontmatter in prompt files.
type promptFrontmatter struct {
	Description string `yaml:"description"`
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

		name := strings.TrimSuffix(entry.Name(), ".md")
		path := filepath.Join("prompts", entry.Name())

		content, err := promptFiles.ReadFile(path)
		if err != nil {
			continue
		}

		description, body := parseFrontmatter(content)

		prompt := &mcp.Prompt{
			Name:        name,
			Description: description,
		}
		s.server.AddPrompt(prompt, makePromptHandler(description, body))
	}
}

// parseFrontmatter extracts YAML frontmatter and returns description and body.
func parseFrontmatter(content []byte) (description string, body string) {
	if !bytes.HasPrefix(content, []byte("---\n")) {
		return "", string(content)
	}

	rest := content[4:]
	end := bytes.Index(rest, []byte("\n---\n"))
	if end == -1 {
		return "", string(content)
	}

	var fm promptFrontmatter
	if err := yaml.Unmarshal(rest[:end], &fm); err != nil {
		return "", string(content)
	}

	body = strings.TrimPrefix(string(rest[end+5:]), "\n")
	return fm.Description, body
}

func makePromptHandler(description, body string) mcp.PromptHandler {
	return func(ctx context.Context, req *mcp.GetPromptRequest) (*mcp.GetPromptResult, error) {
		return &mcp.GetPromptResult{
			Description: description,
			Messages: []*mcp.PromptMessage{
				{
					Role:    "user",
					Content: &mcp.TextContent{Text: body},
				},
			},
		}, nil
	}
}
