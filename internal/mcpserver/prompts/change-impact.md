---
name: change-impact
title: Change Impact Analysis
description: Analyze blast radius before modifying a file - find dependents, co-change candidates, and reviewers
arguments:
  - name: target
    description: File or function to analyze impact for
    required: true
  - name: paths
    description: Scope of analysis
    required: false
    default: "."
  - name: days
    description: Days of git history for temporal coupling
    required: false
    default: "30"
  - name: min_cochanges
    description: Minimum co-changes for temporal coupling
    required: false
    default: "3"
---

# Change Impact Analysis

Analyze the impact of changing: {{.target}}

## When to Use

- Before refactoring a core module
- When planning breaking API changes
- To understand ripple effects of a bug fix
- Before deprecating or removing code
- When estimating effort for a change

## Workflow

### Step 1: Direct Dependencies
```
analyze_graph:
  paths: {{.paths}}
  scope: function
  include_metrics: true
```
Find what directly imports, calls, or extends `{{.target}}`.
Check PageRank - high centrality means more careful changes needed.

### Step 2: Temporal Coupling
```
analyze_temporal_coupling:
  paths: {{.paths}}
  days: {{.days}}
  min_cochanges: {{.min_cochanges}}
```
Find files that historically change together with `{{.target}}`.
These likely need updates even without direct dependencies.

### Step 3: Code Ownership
```
analyze_ownership:
  paths: {{.paths}}
```
Identify who should review changes to `{{.target}}` and its dependents.

### Step 4: Complexity Assessment
```
analyze_complexity:
  paths: [{{.target}}]
  functions_only: true
```
Understand current complexity of `{{.target}}` to gauge refactoring risk.

## Impact Factors

| Factor | Risk Level | Implication |
|--------|------------|-------------|
| PageRank > 0.01 | High | Central code, many indirect dependents |
| Direct dependents > 10 | High | Wide API surface |
| Temporal coupling > 5 files | Medium | Hidden dependencies |
| Single owner | Medium | Knowledge concentration |
| Cognitive complexity > 15 | Medium | Hard to modify safely |

## Output

### Impact Summary for {{.target}}

**Risk Level**: [LOW | MEDIUM | HIGH | CRITICAL]
**PageRank**: [score] (higher = more central)
**Direct Dependents**: [count]
**Likely Co-Changes**: [count]

### Centrality Analysis

| Metric | Value | Interpretation |
|--------|-------|----------------|
| PageRank | | Importance in call graph |
| In-degree | | Direct callers/importers |
| Out-degree | | Direct dependencies |
| Betweenness | | Bridge between components |

### Direct Dependents

Files/functions that directly use `{{.target}}`:

| Dependent | Type | File | Line | Usage |
|-----------|------|------|------|-------|
| | import/call/extend | | | |

### Transitive Impact

Estimate of total affected files through dependency chain:
| Depth | Files Affected | Notable Components |
|-------|----------------|-------------------|
| 1 (direct) | | |
| 2 | | |
| 3+ | | |

### Historical Co-Changes

Files that typically change when `{{.target}}` changes:

| File | Co-change Count | Confidence | Has Import? |
|------|-----------------|------------|-------------|
| | | | Yes/No |

Files with "No" in the import column indicate hidden dependencies - these need special attention.

### Recommended Reviewers

Based on code ownership of `{{.target}}` and its dependents:

| Reviewer | Area | Ownership % | Required |
|----------|------|-------------|----------|
| | {{.target}} | | Yes |
| | dependents | | Recommended |

### Change Checklist

Before modifying `{{.target}}`:

- [ ] Update all direct dependents listed above
- [ ] Check temporal coupling files for necessary updates
- [ ] Notify reviewers listed above
- [ ] Add/update tests for affected paths
- [ ] Consider feature flag for high-risk changes
- [ ] Plan rollback strategy if PageRank > 0.01

### Risk Assessment

**Verdict**: [SAFE | CAUTION | HIGH-RISK | CRITICAL]

[Summary of key risks and mitigation strategies]
