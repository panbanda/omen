package mcpserver

import (
	"encoding/json"
	"strings"
)

// Manifest represents the MCP server manifest (server.json) format.
// Uses schema version 2025-10-17 with camelCase field names.
type Manifest struct {
	Schema        string         `json:"$schema"`
	Name          string         `json:"name"`
	Description   string         `json:"description"`
	Repository    Repository     `json:"repository"`
	VersionDetail VersionDetail  `json:"versionDetail"`
	Packages      []Package      `json:"packages"`
	Remotes       []any          `json:"remotes"`
	Tools         []ManifestTool `json:"tools"`
}

// Repository contains source repository information.
type Repository struct {
	URL    string `json:"url"`
	Source string `json:"source"`
	ID     string `json:"id"`
}

// VersionDetail contains version information.
type VersionDetail struct {
	Version string `json:"version"`
}

// Package describes how to install/run the MCP server.
type Package struct {
	RegistryType         string    `json:"registryType"`
	Identifier           string    `json:"identifier"`
	Version              string    `json:"version"`
	EnvironmentVariables []string  `json:"environmentVariables"`
	Transport            Transport `json:"transport"`
}

// Transport describes the communication method.
type Transport struct {
	Type  string          `json:"type"`
	Stdio *StdioTransport `json:"stdio,omitempty"`
}

// StdioTransport contains stdio-specific configuration.
type StdioTransport struct {
	Args []string `json:"args"`
}

// ManifestTool describes a tool in the manifest.
type ManifestTool struct {
	Name        string `json:"name"`
	Description string `json:"description"`
}

// toolDefinitions returns all tool names and their short descriptions.
// The description is extracted from the first line of the full description.
func toolDefinitions() []ManifestTool {
	tools := []struct {
		name        string
		description func() string
	}{
		{"analyze_complexity", describeComplexity},
		{"analyze_satd", describeSATD},
		{"analyze_deadcode", describeDeadcode},
		{"analyze_churn", describeChurn},
		{"analyze_duplicates", describeDuplicates},
		{"analyze_defect", describeDefect},
		{"analyze_tdg", describeTDG},
		{"analyze_graph", describeGraph},
		{"analyze_hotspot", describeHotspot},
		{"analyze_temporal_coupling", describeTemporalCoupling},
		{"analyze_ownership", describeOwnership},
		{"analyze_cohesion", describeCohesion},
		{"analyze_repo_map", describeRepoMap},
		{"analyze_smells", describeSmells},
		{"analyze_changes", describeChanges},
		{"analyze_flags", describeFlags},
	}

	result := make([]ManifestTool, len(tools))
	for i, t := range tools {
		desc := t.description()
		// Extract first line as short description
		if idx := strings.Index(desc, "\n"); idx > 0 {
			desc = desc[:idx]
		}
		result[i] = ManifestTool{
			Name:        t.name,
			Description: desc,
		}
	}
	return result
}

// GenerateManifest creates the MCP server manifest JSON.
func GenerateManifest(version string) ([]byte, error) {
	if version == "" {
		version = "0.0.0"
	}

	manifest := Manifest{
		Schema:      "https://static.modelcontextprotocol.io/schemas/2025-10-17/server.schema.json",
		Name:        "io.github.panbanda/omen",
		Description: "Multi-language code analysis for complexity, debt, hotspots, ownership, and defect prediction",
		Repository: Repository{
			URL:    "https://github.com/panbanda/omen",
			Source: "github",
			ID:     "panbanda/omen",
		},
		VersionDetail: VersionDetail{
			Version: version,
		},
		Packages: []Package{
			{
				RegistryType:         "oci",
				Identifier:           "ghcr.io/panbanda/omen",
				Version:              version,
				EnvironmentVariables: []string{},
				Transport: Transport{
					Type: "stdio",
					Stdio: &StdioTransport{
						Args: []string{"mcp"},
					},
				},
			},
		},
		Remotes: []any{},
		Tools:   toolDefinitions(),
	}

	return json.MarshalIndent(manifest, "", "  ")
}
