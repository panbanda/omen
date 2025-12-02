---
name: find-bugs
title: Find Bugs
description: Locate likely bug locations using defect prediction, hotspot analysis, and temporal coupling. Use when investigating bugs with unclear location, reviewing high-risk code, or prioritizing where to look first.
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: days
    description: Days of git history to consider
    required: false
    default: "30"
  - name: focus
    description: "Focus area: security, performance, reliability, or all"
    required: false
    default: "all"
  - name: top
    description: Maximum items to return
    required: false
    default: "20"
  - name: min_cochanges
    description: Minimum co-changes for temporal coupling
    required: false
    default: "3"
---

# Bug Hunt

Identify the most likely locations for bugs in: {{.paths}}

## When to Use

- After receiving a bug report (narrow the search)
- Before a release (proactive bug hunting)
- During security audits
- When investigating intermittent failures

## Workflow

### Step 1: Statistical Defect Prediction
```
analyze_defect:
  paths: {{.paths}}
  high_risk_only: false
```
Get file-level defect probability based on churn, complexity, duplication, and coupling.

### Step 2: Hotspot Analysis
```
analyze_hotspot:
  paths: {{.paths}}
  days: {{.days}}
  top: {{.top}}
```
Find files with both high churn AND high complexity - the intersection is where bugs hide.

### Step 3: Temporal Coupling
```
analyze_temporal_coupling:
  paths: {{.paths}}
  days: {{.days}}
  min_cochanges: {{.min_cochanges}}
```
Find files that change together - bugs often span coupled files.

### Step 4: Knowledge Silos
```
analyze_ownership:
  paths: {{.paths}}
  top: {{.top}}
```
Single-owner files have higher bug risk (no peer review, bus factor).

### Step 5: Explicit Debt Markers
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: false
```
Find TODO, FIXME, HACK, BUG markers - explicit acknowledgment of issues.

## Risk Indicators

Files are more likely to contain bugs when they have:

| Indicator | Threshold | Weight |
|-----------|-----------|--------|
| Defect probability | > 0.7 | High |
| Hotspot score | > 0.5 | High |
| Single owner | 100% ownership | Medium |
| Strong temporal coupling | > 5 co-changes | Medium |
| SATD markers (BUG, FIXME) | Any | Medium |
| Security SATD | Any | Critical |

## Output

### Bug Hunt Summary

**Analysis Scope**: {{.paths}}
**History Window**: {{.days}} days
**Focus**: {{.focus}}

### High-Risk Files (Defect Probability > 0.7)

| Rank | File | Probability | Primary Factor | Secondary Factors |
|------|------|-------------|----------------|-------------------|
| 1 | | | | |

### Hotspots (High Churn + High Complexity)

| File | Hotspot Score | Churn Score | Complexity Score | Severity |
|------|---------------|-------------|------------------|----------|

### Knowledge Silos (Bus Factor = 1)

| File | Owner | Lines | Risk |
|------|-------|-------|------|

### Temporal Coupling Clusters

Files that change together (bugs may span these):
| Cluster | Files | Co-changes | Concern |
|---------|-------|------------|---------|

### Explicit Bug Markers

| File | Line | Marker | Content | Severity |
|------|------|--------|---------|----------|

### Investigation Priorities

Based on the analysis, investigate in this order:

1. **Critical** (investigate immediately):
   - [Files with defect probability > 0.8 AND hotspot score > 0.6]

2. **High** (investigate soon):
   - [Files with defect probability > 0.7 OR hotspot score > 0.5]

3. **Medium** (add to backlog):
   - [Single-owner hotspots, temporal coupling clusters]

### Testing Recommendations

Files that need additional test coverage based on risk:
| File | Current Risk | Recommended Tests |
|------|--------------|-------------------|
