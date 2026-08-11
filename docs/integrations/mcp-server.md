---
sidebar_position: 1
---

# MCP Server

```bash
omen mcp
```

Omen includes a built-in [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server that exposes all analyzers as tools for LLMs. This allows AI assistants to query code analysis results during planning, code generation, and review workflows -- giving them structural awareness of the codebase that they otherwise lack.

## Why MCP

LLMs generate code without knowing the structural context of the codebase they are modifying. They cannot see that a module already has cyclomatic complexity of 25, that a function was changed 47 times in the last 6 months, or that a function is still a `todo!()` stub. MCP gives them access to this information through a standardized tool-calling interface.

When connected to Omen's MCP server, an AI assistant can check complexity before adding a new branch, look for code clones before writing a function that might already exist, and assess defect risk before modifying a file that has historically been bug-prone.

## Available Tools

`McpServer::tool_names()` is the single source of truth for the tool list; the manifest reads from it. The server currently exposes these tools:

| Tool | Description |
|------|-------------|
| `context` | Agent-oriented repository overview and navigation hints |
| `outline` | Token-cheap imports, classes, and function outline for a file |
| `complexity` | Cyclomatic and cognitive complexity per function |
| `satd` | Self-admitted technical debt in comments |
| `stubs` | Incomplete/placeholder implementation detection (read-only; does not expose the CLI's `--gate` option) |
| `deadcode` | Unreachable functions and unused exports |
| `churn` | File change frequency and volume |
| `clones` | Duplicated code blocks (Type 1, 2, 3) |
| `defect` | Defect probability predictions |
| `changes` | Risk scores for recent modifications (JIT) |
| `diff` | Structural analysis of uncommitted or branch changes |
| `tdg` | Technical debt gradient scores |
| `graph` | Import/dependency relationships and cycles |
| `hotspot` | Files with high complexity and high churn |
| `temporal` | Files that change together (hidden dependencies) |
| `ownership` | Contributor distribution and bus factor |
| `cohesion` | Chidamber-Kemerer OO metrics (WMC, CBO, RFC, LCOM4) |
| `repomap` | Structural map of modules, symbols, relationships |
| `smells` | Architectural code smells |
| `flags` | Feature flag usage and staleness |
| `score` | Composite repository health score (0-100) |
| `semantic_search` | Natural language code search with optional complexity filtering |
| `get_symbol` | Source, location, callers/callees, and complexity for one symbol |
| `impact` | Transitive caller/callee blast-radius analysis for a symbol |
| `semantic_search_hyde` | HyDE-style search using a hypothetical code snippet as query |

Each tool honors the parameters advertised in its MCP input schema and returns structured results. Every tool also accepts `limit` (default 50) and `offset` (default 0) for pagination. The `semantic_search` and `semantic_search_hyde` tools additionally support `max_complexity` (filter out high-complexity functions) and `include_projects` (cross-repo search) parameters.

## Output Format

Tool responses are serialized as compact (minified, single-line) JSON, wrapped in an envelope containing `tool`, `total_items`, `returned`, `offset`, and `result` (plus `git_skipped_reason` when applicable). Some tools can return agent-facing Markdown instead when their advertised `format` parameter allows it. Compact JSON keeps token usage low without losing any structure -- everything available in the full JSON output is still present, just without extra whitespace.

## Transport and Filesystem Scope

The server uses stdio transport only:

```bash
omen mcp
```

There are no host or port options -- MCP communication happens over stdin/stdout, so there is nothing to bind or expose on the network.

By default, tool paths are confined to the configured repository root. Pass `--allow-external-paths` to opt out of that boundary for clients that intentionally need broader filesystem access:

```bash
omen mcp --allow-external-paths
```

## Setup

### Claude Desktop

Add Omen to your Claude Desktop MCP configuration. Edit `claude_desktop_config.json`:

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

On macOS, this file is located at `~/Library/Application Support/Claude/claude_desktop_config.json`. On Linux, `~/.config/Claude/claude_desktop_config.json`.

If Omen is installed via Homebrew or Cargo and is on your PATH, the `command` field just needs `"omen"`. If you installed it elsewhere, use the full path to the binary.

### Claude Code

Register Omen as an MCP server using the Claude Code CLI:

```bash
claude mcp add omen -- omen mcp
```

This registers the server for the current project. Verify it's available:

```bash
claude mcp list
```

You should see `omen` in the list of configured servers. The tools will be available in subsequent Claude Code sessions.

Installing the [Claude Code plugin](./claude-code-plugin.md) registers this MCP server automatically, so manual setup is only needed if you want to configure it by hand.

### Other MCP Clients

Any MCP-compatible client can connect to Omen. The server communicates over stdio transport. Start it with:

```bash
omen mcp
```

The server reads JSON-RPC messages from stdin and writes responses to stdout, following the MCP specification.

## Example Queries

Once connected, an LLM can call Omen tools naturally during conversation. Here are examples of what users can ask and how the LLM will use the tools:

**"What are the most complex functions in this codebase?"**
The LLM calls the `complexity` tool and summarizes the results, highlighting functions that exceed thresholds.

**"Is it safe to modify `src/auth/session.rs`?"**
The LLM can call `churn` (how often this file changes), `ownership` (who knows this file), `defect` (defect probability), and `hotspot` (is it a hotspot) to give a risk assessment.

**"Are there any unfinished stubs left in this module?"**
The LLM calls `stubs` and reports any `todo!()`/`NotImplementedError`-style placeholders or elided implementations.

**"Are there any security concerns in the comments?"**
The LLM calls `satd` and filters for security-category items.

**"Find code similar to this function."**
The LLM calls `semantic_search` with a natural language description of the function's purpose.

**"What's the overall health of this project?"**
The LLM calls `score` and breaks down the component scores.

**"Show me the dependency graph for the auth module."**
The LLM calls `graph` with the path scoped to the auth module.

**"Are there any stale feature flags?"**
The LLM calls `flags` and filters for stale flags.

**"What would break if I changed the `User` struct?"**
The LLM calls `impact` for transitive callers/callees, `temporal` for files that co-change with the User module, and `clones` to check for duplicated logic that might need parallel changes.

**"Give me a quick orientation to this repository."**
The LLM calls `context` for an agent-oriented overview, and `outline` on specific files for a token-cheap map of imports, classes, and functions before reading full source.

## Programmatic Usage

The MCP server returns structured data that can be parsed by any MCP client. A typical tool call and response cycle:

**Request** (from LLM via MCP client):
```json
{
  "method": "tools/call",
  "params": {
    "name": "complexity",
    "arguments": {
      "path": "./src",
      "limit": 50,
      "offset": 0
    }
  }
}
```

**Response** (from Omen MCP server):
The response contains the analysis results as compact JSON, wrapped in the pagination envelope described above.

## Running Alongside Other MCP Servers

Omen's MCP server can run alongside other MCP tools. Each MCP server is a separate process, and the client manages connections to all of them. There are no port conflicts because MCP uses stdio transport.

```json
{
  "mcpServers": {
    "omen": {
      "command": "omen",
      "args": ["mcp"]
    },
    "other-tool": {
      "command": "other-tool",
      "args": ["serve"]
    }
  }
}
```
