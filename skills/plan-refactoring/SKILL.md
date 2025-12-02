---
name: plan-refactoring
description: Identify highest-priority refactoring targets based on TDG scores, complexity, and code clones. Use when planning refactoring work, prioritizing tech debt, or finding quick wins for code quality.
---

# Plan Refactoring

Identify the highest-value refactoring targets by combining multiple code quality signals into a prioritized list.

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

### Step 1: Get TDG Hotspots

Use the `analyze_tdg` tool for Technical Debt Gradient scores:

```
analyze_tdg(paths: ["."], hotspots: 10)
```

TDG combines complexity, churn, and code health into a 0-100 score with letter grades (A-F).

### Step 2: Analyze Complexity

Use the `analyze_complexity` tool for detailed complexity metrics:

```
analyze_complexity(paths: ["."], functions_only: true)
```

Look for functions with:
- Cyclomatic complexity > 15
- Cognitive complexity > 20

### Step 3: Find Code Clones

Use the `analyze_duplicates` tool to find duplicated code:

```
analyze_duplicates(paths: ["."])
```

Clones indicate opportunities for extraction and consolidation.

### Step 4: Check SATD Markers

Use the `analyze_satd` tool for Self-Admitted Technical Debt:

```
analyze_satd(paths: ["."])
```

Look for TODO, FIXME, HACK, and XXX comments that indicate known issues.

### Step 5: Analyze Cohesion

Use the `analyze_cohesion` tool for class/module cohesion:

```
analyze_cohesion(paths: ["."])
```

Low cohesion (LCOM > 0.7) suggests classes doing too many things.

## Prioritization Matrix

Rank refactoring targets by combining signals:

| Priority | Criteria |
|----------|----------|
| Critical | TDG grade F + high churn + SATD markers |
| High | TDG grade D + complexity > 20 + clones |
| Medium | TDG grade C + low cohesion |
| Low | TDG grade B with isolated issues |

## Output Format

Present findings as:

```markdown
# Refactoring Priority Report

## Critical Priority
1. `file.go:FunctionName` (TDG: 25/F)
   - Cyclomatic: 28, Cognitive: 35
   - 3 SATD markers (TODO, HACK)
   - Clone of `other_file.go:OtherFunction`
   - **Recommendation**: Extract helper functions, consolidate clones

## High Priority
...

## Suggested Approach
1. Start with X because...
2. Then address Y which will also fix...
```

## Quick Wins

Focus on these for fast improvements:
1. **Consolidate clones**: Extract duplicated code into shared functions
2. **Fix SATD**: Address TODO/FIXME comments, especially in hot files
3. **Split large functions**: Break down functions with cyclomatic > 20
4. **Improve cohesion**: Split classes with LCOM > 0.8
