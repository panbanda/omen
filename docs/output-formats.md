---
sidebar_position: 20
---

# Output Formats

All Omen commands support three output formats, controlled by the `-f` or `--format` flag:

```bash
omen -f markdown complexity
omen -f json score
omen -f text churn
```

The default format is markdown.

## Markdown (default)

Human-readable output using tables and structured headings. This is the default when running Omen in a terminal and produces output suitable for pasting into documentation, pull request comments, or issue trackers.

```bash
omen complexity
# equivalent to:
omen -f markdown complexity
```

Example output:

```markdown
# Complexity Analysis

## Summary

| Metric              | Value |
|---------------------|-------|
| Files analyzed      | 42    |
| Functions analyzed  | 318   |
| Warnings            | 12    |
| Errors              | 3     |

## Functions Exceeding Thresholds

| File                          | Function              | Cyclomatic | Cognitive | Nesting |
|-------------------------------|-----------------------|------------|-----------|---------|
| src/analyzer/complexity.rs    | analyze_function      | 24         | 31        | 5       |
| src/git/blame.rs              | compute_ownership     | 22         | 28        | 6       |
| src/scoring/composite.rs      | calculate_score       | 18         | 19        | 4       |
```

Markdown output renders cleanly in terminals that support Unicode box-drawing characters and in any Markdown viewer.

## JSON

Machine-parseable output with full nesting. Use this for CI/CD pipelines, scripting, or programmatic consumption. This is also the format used by the MCP server.

```bash
omen -f json complexity
```

Example output:

```json
{
  "summary": {
    "files_analyzed": 42,
    "functions_analyzed": 318,
    "warnings": 12,
    "errors": 3
  },
  "functions": [
    {
      "file": "src/analyzer/complexity.rs",
      "function": "analyze_function",
      "line": 45,
      "cyclomatic": 24,
      "cognitive": 31,
      "max_nesting": 5,
      "level": "error"
    },
    {
      "file": "src/git/blame.rs",
      "function": "compute_ownership",
      "line": 112,
      "cyclomatic": 22,
      "cognitive": 28,
      "max_nesting": 6,
      "level": "error"
    }
  ]
}
```

JSON output is stable and suitable for piping to tools like `jq`:

```bash
# Extract just the function names with errors
omen -f json complexity | jq '.functions[] | select(.level == "error") | .function'

# Get the repository score as a number
omen -f json score | jq '.overall_score'
```

### Compact JSON

The global `--compact` flag minifies JSON output to a single line. It is a clap global flag, so it may appear before or after the subcommand, and it is ignored for non-JSON formats:

```bash
omen -f json score --compact
omen --compact -f json complexity
```

This is the format used for MCP tool responses and is useful whenever token efficiency matters more than human readability -- for example, when piping results into an LLM prompt or storing them as a CI artifact.

## Text

Plain ASCII output with minimal formatting. No tables, no Markdown syntax, no Unicode characters. Useful in environments where terminal rendering is limited or when output will be processed by simple text tools.

```bash
omen -f text complexity
```

Example output:

```
Complexity Analysis
Files analyzed: 42
Functions analyzed: 318
Warnings: 12
Errors: 3

Functions exceeding thresholds:

  src/analyzer/complexity.rs:45  analyze_function  cyclomatic=24  cognitive=31  nesting=5  ERROR
  src/git/blame.rs:112  compute_ownership  cyclomatic=22  cognitive=28  nesting=6  ERROR
  src/scoring/composite.rs:89  calculate_score  cyclomatic=18  cognitive=19  nesting=4  WARN
```

## Format Comparison

| Format   | Approximate Token Count | Best For |
|----------|------------------------|----------|
| Markdown | 100% (baseline)        | Human reading, documentation, PR comments |
| JSON     | 120-140%               | CI/CD, scripting, programmatic access |
| Text     | 70-80%                 | Minimal environments, simple text processing |
| JSON + `--compact` | 90-110%      | MCP tool responses, LLM prompts, token-constrained contexts |

## Pagination for Large Results

Most analyzers support `--top N` and `--offset N` on the CLI to page through large result sets. The MCP server uses an equivalent `limit`/`offset` envelope on every tool call (default `limit`: 50): the response wraps the analysis result together with `tool`, `total_items`, `returned`, and `offset` fields (plus `git_skipped_reason` when applicable), so a client can request additional pages without re-running the full analysis. See [MCP Server](./integrations/mcp-server.md) for details.

## Setting a Default Format

The format can be set per-invocation with `-f`. There is no configuration file option to change the default format -- it is always markdown for CLI usage. This keeps behavior predictable: running `omen complexity` in a terminal always produces the same format regardless of project configuration.
