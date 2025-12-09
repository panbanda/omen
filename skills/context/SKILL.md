---
name: context
description: Get deep context for a codebase, file, or symbol. Use when you need to understand complexity, dependencies, and technical debt before modifying code.
---

# Context

Get deep context for a codebase, specific file, or symbol. Use this skill when you need to understand code before making changes.

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

## When to Use

- Before modifying unfamiliar code
- When debugging and need to understand a function's complexity
- Before refactoring to assess risk and debt
- When you need focused metrics instead of project-wide analysis

## Workflow

### Step 1: Get Context

Use the `get_context` tool with your target:

```
get_context(focus: "path/to/file.go")
```

The tool accepts multiple input formats:
- **Exact path**: `src/service/user.go`
- **Glob pattern**: `src/**/*_test.go`
- **Basename**: `user.go` (searches all directories)
- **Symbol name**: `CreateUser` (requires repo map)

### Step 2: Handle Ambiguous Matches

If your input matches multiple files or symbols, you'll receive candidates:

```
error: ambiguous match
candidates:
  - path: pkg/a/service.go
  - path: pkg/b/service.go
```

Retry with a more specific path:

```
get_context(focus: "pkg/a/service.go")
```

### Step 3: Interpret Results

For files, you'll receive:
- **Complexity metrics**: Cyclomatic and cognitive totals, per-function breakdown
- **Technical debt**: SATD markers (TODO, FIXME, HACK) with locations

For symbols, you'll receive:
- **Definition**: File, line, and kind (function/method/type)
- **Complexity**: Metrics for that specific function

## Complexity Thresholds

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Cyclomatic (per function) | <10 | 10-20 | >20 |
| Cognitive (per function) | <15 | 15-30 | >30 |

## Technical Debt Severity

| Marker | Severity | Meaning |
|--------|----------|---------|
| SECURITY, VULN | Critical | Security issue - address before changes |
| FIXME, BUG | High | Known defect - consider fixing |
| HACK, REFACTOR | Medium | Design compromise |
| TODO, NOTE | Low | Future work |

## Output Format

Present focused context as:

```markdown
# Context: `path/to/file.go`

## Complexity Overview
- **Total Cyclomatic**: 45
- **Total Cognitive**: 62
- **Risk Level**: MEDIUM

## Functions

| Function | Line | Cyclomatic | Cognitive | Status |
|----------|------|------------|-----------|--------|
| CreateUser | 25 | 8 | 12 | OK |
| ValidateInput | 50 | 15 | 22 | WARNING |
| ProcessOrder | 100 | 25 | 35 | CRITICAL |

## Technical Debt

| Line | Type | Content |
|------|------|---------|
| 72 | TODO | Add input validation |
| 105 | FIXME | Race condition in concurrent access |
| 150 | HACK | Workaround for API bug |

## Recommendations

1. **ProcessOrder** exceeds complexity thresholds - consider splitting
2. **FIXME on line 105** indicates a known bug - review before changes
3. Consider addressing TODO on line 72 while modifying this file
```

## Combining with Other Analysis

For deeper context, combine with other tools:

- Use `analyze_ownership` to find who knows this code
- Use `analyze_graph` to see what depends on it
- Use `analyze_temporal_coupling` to find implicit dependencies
