---
name: plan-refactoring
description: Identify highest-ROI refactoring targets
usage: /plan-refactoring [path]
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
---

# Plan Refactoring Skill

Find refactoring targets in: `{{.paths}}`

## Quick Start

```bash
# Find hotspots (high churn + complexity)
omen hotspot -n 10 -f json

# Find code clones
omen clones --min-tokens 50 -f json

# Find acknowledged debt
omen satd -f json | jq '.items[] | select(.category == "design")'
```

## Priority Matrix

| Finding | Effort | ROI |
|---------|--------|-----|
| Hotspot with clones | Medium | High |
| God component | High | High |
| Isolated clone | Low | Medium |
| Low-churn debt | Low | Low |

## Effort Guide

- Extract function: 30min - 2hr
- Split class: 2hr - 1 day
- Break cycle: 1-3 days
- Extract module: 1-2 weeks

## Sequencing

1. Quick wins: hotspot + low complexity
2. Clone extraction
3. God components (dedicated sprint)
