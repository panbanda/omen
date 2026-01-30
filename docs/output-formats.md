---
sidebar_position: 20
---

# Output Formats

All Omen commands support four output formats, controlled by the `-f` or `--format` flag:

```bash
omen -f markdown complexity
omen -f json score
omen -f text churn
omen -f toon all
```

The default format is markdown. The MCP server defaults to TOON.

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

Machine-parseable output with full nesting. Use this for CI/CD pipelines, scripting, or programmatic consumption.

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
omen -f json score | jq '.score'
```

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

## TOON

Token-Oriented Object Notation (TOON) is a compact structured format designed for LLM workflows. It is 30-60% smaller than equivalent JSON while maintaining high comprehension accuracy for language models.

```bash
omen -f toon complexity
```

Example output:

```
@complexity_analysis
  summary{files:42 functions:318 warnings:12 errors:3}
  #functions
    [src/analyzer/complexity.rs:45|analyze_function|cyc:24|cog:31|nest:5|ERROR]
    [src/git/blame.rs:112|compute_ownership|cyc:22|cog:28|nest:6|ERROR]
    [src/scoring/composite.rs:89|calculate_score|cyc:18|cog:19|nest:4|WARN]
```

TOON reduces token consumption when analysis results are fed into LLM prompts, which matters for cost and context window utilization. It uses delimiters (`@`, `#`, `{}`, `[]`, `|`) instead of verbose JSON syntax (quoted keys, colons, commas, braces).

The MCP server uses TOON as its default output format because MCP responses are consumed by language models, where token efficiency directly impacts cost and available context.

For the TOON specification, see [github.com/toon-format/toon](https://github.com/toon-format/toon).

## Format Comparison

The same data in all four formats, showing relative verbosity:

| Format   | Approximate Token Count | Best For |
|----------|------------------------|----------|
| Markdown | 100% (baseline)        | Human reading, documentation, PR comments |
| JSON     | 120-140%               | CI/CD, scripting, programmatic access |
| Text     | 70-80%                 | Minimal environments, simple text processing |
| TOON     | 40-60%                 | LLM consumption, MCP server, token-constrained contexts |

## Setting a Default Format

The format can be set per-invocation with `-f`. There is no configuration file option to change the default format -- it is always markdown for CLI usage and TOON for MCP. This keeps behavior predictable: running `omen complexity` in a terminal always produces the same format regardless of project configuration.
