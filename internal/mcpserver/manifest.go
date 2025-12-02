package mcpserver

import (
	"encoding/json"
)

// Manifest represents the MCP server manifest (server.json) format.
// Uses schema version 2025-10-17.
type Manifest struct {
	Schema      string      `json:"$schema"`
	Name        string      `json:"name"`
	Description string      `json:"description"`
	Version     string      `json:"version"`
	Repository  *Repository `json:"repository,omitempty"`
	Packages    []Package   `json:"packages,omitempty"`
}

// Repository contains source repository information.
type Repository struct {
	URL    string `json:"url"`
	Source string `json:"source"`
	ID     string `json:"id,omitempty"`
}

// Package describes how to install/run the MCP server.
type Package struct {
	RegistryType     string     `json:"registryType"`
	Identifier       string     `json:"identifier"`
	PackageArguments []Argument `json:"packageArguments,omitempty"`
	Transport        Transport  `json:"transport"`
}

// Argument represents a command-line argument.
type Argument struct {
	Type  string `json:"type"`
	Value string `json:"value,omitempty"`
}

// Transport describes the communication method.
type Transport struct {
	Type string `json:"type"`
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
		Version:     version,
		Repository: &Repository{
			URL:    "https://github.com/panbanda/omen",
			Source: "github",
		},
		Packages: []Package{
			{
				RegistryType: "oci",
				Identifier:   "ghcr.io/panbanda/omen:" + version,
				PackageArguments: []Argument{
					{Type: "positional", Value: "mcp"},
				},
				Transport: Transport{
					Type: "stdio",
				},
			},
		},
	}

	return json.MarshalIndent(manifest, "", "  ")
}
