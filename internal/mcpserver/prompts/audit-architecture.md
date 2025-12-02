---
name: audit-architecture
title: Audit Architecture
description: Audit module coupling, cohesion, hidden dependencies, and design smells. Use when conducting architecture reviews, evaluating design decisions, or identifying structural tech debt.
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: scope
    description: "Analysis scope: file, function, module, or package"
    required: false
    default: "module"
  - name: include_temporal
    description: Include temporal coupling analysis (requires git)
    required: false
    default: "true"
  - name: top
    description: Maximum items to return
    required: false
    default: "20"
  - name: days
    description: Days of git history for temporal coupling
    required: false
    default: "30"
  - name: min_cochanges
    description: Minimum co-changes for temporal coupling
    required: false
    default: "3"
---

# Architecture Review

Analyze the architectural health of the codebase at: {{.paths}}

## When to Use

- During architectural design reviews
- Before major refactoring efforts
- When onboarding to understand system structure
- Periodic health checks (monthly/quarterly)

## Workflow

### Step 1: Dependency Graph Analysis
```
analyze_graph:
  paths: {{.paths}}
  scope: {{.scope}}
  include_metrics: true
```
Get module dependencies, cycles, and centrality metrics (PageRank, betweenness).

### Step 2: Architectural Smell Detection
```
analyze_smells:
  paths: {{.paths}}
```
Detect cyclic dependencies, hub components, god components, and unstable dependencies.

### Step 3: Class-Level Design Quality
```
analyze_cohesion:
  paths: {{.paths}}
  sort: lcom
  top: {{.top}}
```
Get CK metrics (LCOM, WMC, CBO, DIT, RFC) for OO design quality.

### Step 4: Hidden Dependencies (if git available)
```
analyze_temporal_coupling:
  paths: {{.paths}}
  days: {{.days}}
  min_cochanges: {{.min_cochanges}}
```
Find files that change together but have no import relationship - indicates missing abstractions.

### Step 5: Ownership Alignment
```
analyze_ownership:
  paths: {{.paths}}
```
Check if code ownership aligns with module boundaries (Conway's Law).

## Thresholds

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Cyclic dependencies | 0 | 1-2 | 3+ |
| LCOM (lack of cohesion) | < 0.5 | 0.5-0.8 | > 0.8 |
| WMC (weighted methods) | < 20 | 20-50 | > 50 |
| CBO (coupling) | < 10 | 10-20 | > 20 |
| DIT (inheritance depth) | < 4 | 4-6 | > 6 |
| Hub components | 0 | 1-2 | 3+ |
| God components | 0 | 1 | 2+ |

## Output

### Architecture Health Summary

**Overall Health**: [GOOD | FAIR | POOR | CRITICAL]
**Scope Analyzed**: {{.paths}} at {{.scope}} level

### Dependency Analysis

| Metric | Value | Status |
|--------|-------|--------|
| Total modules | | |
| Total dependencies | | |
| Dependency cycles | | |
| Graph density | | |
| Avg path length | | |

### Architectural Smells

| Smell | Count | Severity | Components |
|-------|-------|----------|------------|
| Cyclic Dependency | | | |
| Hub Component | | | |
| God Component | | | |
| Unstable Dependency | | | |

### Design Quality (Top Classes by LCOM)

| Class | File | LCOM | WMC | CBO | DIT | Issue |
|-------|------|------|-----|-----|-----|-------|

### Hidden Dependencies

File pairs with high temporal coupling but no import relationship:
| File A | File B | Co-changes | Suggestion |
|--------|--------|------------|------------|

### Conway's Law Analysis

Modules with ownership that doesn't match team boundaries:
| Module | Primary Owner | Secondary Owners | Concern |
|--------|---------------|------------------|---------|

### Recommendations

1. **Immediate** (blocking issues):
   - [List cycles to break, god components to split]

2. **Short-term** (next sprint):
   - [Hub reduction, cohesion improvements]

3. **Long-term** (roadmap):
   - [Architectural refactoring suggestions]
