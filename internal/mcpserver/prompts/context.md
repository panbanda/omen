---
name: context
title: Context
description: Get deep context for a specific file or symbol before making changes. Use when you need to understand a file's complexity, dependencies, and technical debt before modifying it.
arguments:
  - name: focus
    description: File path, glob pattern, basename, or symbol name to focus on
    required: true
  - name: paths
    description: Repository root for context
    required: false
    default: "."
---

# Context

Get deep context for: {{.focus}}

## When to Use

- Before modifying a file you're unfamiliar with
- When debugging and need to understand function complexity
- Before refactoring to assess risk
- To check for technical debt markers before touching code
- When you need focused analysis instead of whole-codebase metrics

## Workflow

### Step 1: Get Context
```
get_context:
  focus: {{.focus}}
  paths: {{.paths}}
```

This resolves your target using this order:
1. **Exact file path** - if file exists at the path
2. **Glob pattern** - if contains *, ?, or [ characters
3. **Basename search** - if looks like a filename (has extension)
4. **Symbol search** - if matches a function/type/method name

### Step 2: Handle Ambiguous Matches

If multiple matches are found, you'll receive candidates like:
```
error: ambiguous match: multiple files or symbols found
candidates:
  - path: pkg/a/service.go
  - path: pkg/b/service.go
```

Retry with a more specific path:
```
get_context:
  focus: pkg/a/service.go
  paths: {{.paths}}
```

## Understanding the Output

### For Files

| Field | Meaning |
|-------|---------|
| Target.Type | "file" |
| Target.Path | Full path to the file |
| Complexity.CyclomaticTotal | Sum of all function cyclomatic complexity |
| Complexity.CognitiveTotal | Sum of all function cognitive complexity |
| Complexity.TopFunctions | Per-function breakdown |
| SATD | Technical debt markers (TODO, FIXME, HACK) |

### For Symbols

| Field | Meaning |
|-------|---------|
| Target.Type | "symbol" |
| Target.Symbol.Name | Function/type name |
| Target.Symbol.Kind | function, method, type, etc. |
| Target.Symbol.File | File containing the symbol |
| Target.Symbol.Line | Line number of definition |
| Complexity | Metrics for this specific function |

## Interpreting Results

### Complexity Thresholds

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Cyclomatic (per function) | <10 | 10-20 | >20 |
| Cognitive (per function) | <15 | 15-30 | >30 |
| Max Nesting | <4 | 4-5 | >5 |

### Technical Debt Severity

| Severity | Markers | Action |
|----------|---------|--------|
| Critical | SECURITY, VULN | Address before any changes |
| High | FIXME, BUG | Consider fixing during change |
| Medium | HACK, REFACTOR | Note for future |
| Low | TODO, NOTE | Track in backlog |

## Output

### Context Summary for {{.focus}}

**Type**: [file | symbol]
**Path**: [resolved file path]
**Risk Level**: [LOW | MEDIUM | HIGH]

### Complexity Metrics

| Function | Line | Cyclomatic | Cognitive | Verdict |
|----------|------|------------|-----------|---------|
| | | | | OK/WARN/CRITICAL |

**Total Cyclomatic**: [sum]
**Total Cognitive**: [sum]

### Technical Debt

| Line | Marker | Severity | Content |
|------|--------|----------|---------|
| | TODO/FIXME/HACK | low/medium/high/critical | |

### Risk Assessment

**Verdict**: [SAFE TO MODIFY | USE CAUTION | HIGH RISK]

**Concerns**:
- [List any complexity or debt issues]

**Recommendations**:
- [Suggestions based on findings]
