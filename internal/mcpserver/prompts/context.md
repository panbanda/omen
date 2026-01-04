---
name: context
title: Context
description: Get complexity and debt metrics for a file or symbol before editing. Use before modifying unfamiliar code.
arguments:
  - name: focus
    description: File path, glob pattern, or symbol name
    required: true
  - name: paths
    description: Repository root
    required: false
    default: "."
---

# Context

Get metrics for: {{.focus}}

## Workflow

```
get_context:
  focus: {{.focus}}
  paths: {{.paths}}
```

Resolution order:
1. Exact file path - if file exists
2. Glob pattern - if contains *, ?, [
3. Basename search - if has extension
4. Symbol lookup - if matches function/type name

## Thresholds

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Cyclomatic | <10 | 10-20 | >20 |
| Cognitive | <15 | 15-30 | >30 |

| Debt Marker | Action |
|-------------|--------|
| SECURITY/VULN | Address before changes |
| FIXME/BUG | Consider fixing |
| TODO/HACK | Track only |

## Decision Tree

After getting context:

1. **Cyclomatic > 20**: Refactor before changing
2. **Critical SATD**: Address debt first
3. **Many complex functions**: Read file to understand structure
4. **Clean metrics**: Proceed with change

## Next Steps

- High complexity? `analyze_complexity` for breakdown
- Need callers? `analyze_graph` with function scope
- Who knows this? `analyze_ownership`
