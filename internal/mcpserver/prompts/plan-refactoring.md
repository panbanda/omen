---
name: plan-refactoring
title: Plan Refactoring
description: Identify highest-ROI refactoring targets.
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

# Refactoring Priority

Find targets in: {{.paths}}

## Workflow

### Step 1: Hotspots
```
analyze_hotspot:
  paths: {{.paths}}
  days: {{.days}}
  top: 10
```
High churn + complexity = high ROI refactoring.

### Step 2: Clones
```
analyze_duplicates:
  paths: {{.paths}}
  min_lines: 10
```
Clones > 10 lines should be extracted.

### Step 3: Debt
```
analyze_satd:
  paths: {{.paths}}
```
HACK/FIXME markers = acknowledged debt.

## Decision Criteria

**Prioritize if:**
- Hotspot score > 0.5
- Clone in 3+ locations
- FIXME in hotspot file

**Defer if:**
- Low churn (< 2 changes in 30 days)
- Isolated module
- TODO without FIXME

## Effort Estimation

| Type | Effort |
|------|--------|
| Extract function | 30min - 2hr |
| Split class | 2hr - 1 day |
| Break cycle | 1-3 days |
| Extract module | 1-2 weeks |

## Sequencing

1. Highest hotspot + lowest complexity (quick wins)
2. Extract clones (reduce surface area)
3. God components (dedicated sprint)

## Before Starting

```
get_context:
  focus: [target file]
```
