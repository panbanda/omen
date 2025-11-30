---
name: context-compression
description: |
  Generate a compressed context summary of a codebase for LLM consumption using PageRank-ranked symbols and dependency graphs. Use this skill when starting work on unfamiliar codebases, onboarding to new projects, or preparing context for complex tasks.
---

# Context Compression

Generate a compressed, LLM-optimized summary of a codebase using Omen's PageRank-based symbol ranking and dependency analysis.

## When to Use

- Starting work on an unfamiliar codebase
- Onboarding to a new project
- Preparing context before complex refactoring
- Understanding architecture before making changes
- Creating technical documentation

## Prerequisites

Omen must be available as an MCP server. Add to Claude Code settings:

```json
{
  "mcpServers": {
    "omen": {
      "command": "omen",
      "args": ["mcp"]
    }
  }
}
```

## Workflow

### Step 1: Generate Symbol Map

Use the `analyze_repo_map` tool to get PageRank-ranked symbols:

```
analyze_repo_map(paths: ["."], top: 30)
```

This identifies the most important symbols (functions, types, interfaces) based on how many other symbols reference them.

### Step 2: Generate Dependency Graph

Use the `analyze_graph` tool to understand module relationships:

```
analyze_graph(paths: ["."], scope: "module", include_metrics: true)
```

This produces a Mermaid diagram showing how modules depend on each other.

### Step 3: Synthesize Context

Combine the outputs to create a compressed context summary:

1. **Core Symbols**: List the top 10-15 PageRank symbols with brief descriptions
2. **Module Map**: Describe the high-level module structure from the dependency graph
3. **Entry Points**: Identify main entry points and their purposes
4. **Key Patterns**: Note any architectural patterns visible in the structure

## Output Format

Structure the context summary as:

```markdown
# Codebase Context: <project-name>

## Core Symbols (by importance)
1. `SymbolName` - Brief description of purpose
2. ...

## Module Structure
- `module/` - Description of responsibility
  - Depends on: list of dependencies
  - Depended on by: list of dependents

## Entry Points
- `main.go` / `index.ts` - Primary entry point
- `cmd/` - CLI commands

## Architecture Notes
- Key patterns observed
- Important conventions
```

## Customization

Adjust the number of symbols based on codebase size:
- Small projects (<50 files): `top: 15`
- Medium projects (50-500 files): `top: 30`
- Large projects (>500 files): `top: 50`

For focused analysis, specify paths:
```
analyze_repo_map(paths: ["src/core", "src/services"], top: 20)
```
