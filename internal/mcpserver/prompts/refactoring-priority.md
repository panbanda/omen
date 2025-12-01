---
name: refactoring-priority
title: Refactoring Priority
description: Identify highest-ROI refactoring targets based on hotspots, complexity, duplication, and technical debt
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: days
    description: Days of git history for churn analysis
    required: false
    default: "30"
  - name: max_items
    description: Maximum items to return
    required: false
    default: "20"
  - name: focus
    description: "Focus area: complexity, duplication, debt, or all"
    required: false
    default: "all"
---

# Refactoring Priority

Identify highest-ROI refactoring targets in: {{.paths}}

## When to Use

- During sprint planning to allocate refactoring time
- When deciding what technical debt to address
- Before major feature work in an area
- To justify refactoring to stakeholders

## Workflow

### Step 1: Hotspot Analysis
```
analyze_hotspot:
  paths: {{.paths}}
  days: {{.days}}
  top: {{.max_items}}
```
Find files with both high churn AND high complexity - highest ROI targets.

### Step 2: Complexity Outliers
```
analyze_complexity:
  paths: {{.paths}}
  functions_only: true
```
Find functions with extreme complexity that are hard to maintain.

### Step 3: Code Clones
```
analyze_duplicates:
  paths: {{.paths}}
  min_lines: 6
  threshold: 0.8
```
Find duplication that should be extracted to shared code.

### Step 4: Explicit Debt
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: false
```
Find TODO, FIXME, HACK markers - explicit acknowledgment of debt.

### Step 5: Design Smells
```
analyze_cohesion:
  paths: {{.paths}}
  sort: lcom
  top: {{.max_items}}
```
Find classes with poor cohesion that should be split.

### Step 6: Centrality Check
```
analyze_repo_map:
  paths: {{.paths}}
  top: {{.max_items}}
```
Check PageRank - refactoring central code has more impact.

## Prioritization Formula

**Priority Score** = (Impact x Frequency) / Effort

- **Impact**: PageRank + defect probability
- **Frequency**: Churn rate (how often touched)
- **Effort**: Complexity + coupling

High score = High value, frequently touched, relatively easy to fix

## Output

### Refactoring Priority Report

**Scope**: {{.paths}}
**History Window**: {{.days}} days
**Focus**: {{.focus}}

---

### Executive Summary

| Category | Count | Effort Estimate |
|----------|-------|-----------------|
| Hotspots | | |
| Complex functions | | |
| Code clones | | |
| SATD markers | | |
| Low cohesion classes | | |

### Top Refactoring Targets (by ROI)

| Rank | Target | Type | Priority Score | Impact | Effort |
|------|--------|------|----------------|--------|--------|
| 1 | | | | | |
| 2 | | | | | |
| 3 | | | | | |

### Hotspots (High Churn + High Complexity)

Files where refactoring compounds over time:

| File | Hotspot Score | Commits | Avg Complexity | Recommendation |
|------|---------------|---------|----------------|----------------|
| | | | | |

**Why prioritize hotspots**: These files are touched frequently. Every future change benefits from improved code quality.

### Complexity Hotspots

Functions exceeding complexity thresholds:

| Function | File | Cyclomatic | Cognitive | Nesting | Suggestion |
|----------|------|------------|-----------|---------|------------|
| | | | | | Extract/Split/Simplify |

**Quick wins**: Functions with high complexity but low coupling are easiest to refactor.

### Duplication Targets

Code clones that should be extracted:

| Clone Group | Instances | Lines | Locations | Extraction Target |
|-------------|-----------|-------|-----------|-------------------|
| | | | | |

**Estimated savings**: [lines] lines of code could be eliminated.

### Technical Debt Markers

Explicit debt acknowledged in code:

| Priority | File | Line | Marker | Content | Age |
|----------|------|------|--------|---------|-----|
| High | | | FIXME/HACK | | |
| Medium | | | TODO | | |

### Design Improvements

Classes with poor cohesion (candidates for splitting):

| Class | File | LCOM | WMC | Suggestion |
|-------|------|------|-----|------------|
| | | | | Split into X and Y |

### Refactoring Roadmap

#### Immediate (This Sprint)
- [ ] [Highest ROI item with specific action]
- [ ] [Second item]

#### Short-term (Next 2-4 Sprints)
- [ ] [Medium priority items]

#### Long-term (Roadmap)
- [ ] [Larger architectural improvements]

### Effort Estimates

| Refactoring | Complexity | Risk | Estimated Effort |
|-------------|------------|------|------------------|
| | Low/Med/High | Low/Med/High | hours/days |

### Dependencies

Refactorings that should be done together:
- [X and Y are coupled - refactor together]
- [Z depends on X - do X first]

---

**Next Steps**: Use `change-impact` before starting each refactoring to understand blast radius.
