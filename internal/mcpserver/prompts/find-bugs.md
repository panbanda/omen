---
name: find-bugs
title: Find Bugs
description: Locate likely bug locations using defect prediction and hotspot analysis.
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: days
    description: Days of git history
    required: false
    default: "30"
---

# Bug Hunt

Find likely bug locations in: {{.paths}}

## Workflow

### Step 1: Defect Prediction
```
analyze_defect:
  paths: {{.paths}}
  high_risk_only: true
```
Focus on files with probability > 0.7.

### Step 2: Hotspot Analysis
```
analyze_hotspot:
  paths: {{.paths}}
  days: {{.days}}
```
High churn + high complexity = bug magnets.

### Step 3: Explicit Markers
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: true
```
Find BUG, FIXME, HACK markers.

## Decision Tree

1. **Defect probability > 0.8**: Read file first, check recent changes
2. **Hotspot in top 5**: Review for logic errors
3. **BUG/FIXME markers**: Known issues - check if related
4. **Multiple signals same file**: High confidence - prioritize

## Deep Dive (Optional)

```
analyze_temporal_coupling:
  paths: {{.paths}}
  days: {{.days}}
  min_cochanges: 3
```
Bugs often span coupled files.

## What to Check

1. Read file with `Read` tool
2. Look for cognitive complexity > 20
3. Check error handling paths
4. Review boundary conditions
5. Trace data flow through complex functions
