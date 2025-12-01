---
name: tech-debt-report
title: Technical Debt Report
description: Comprehensive technical debt assessment including explicit markers, structural debt, duplication, and high-risk areas
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: days
    description: Days of git history for context
    required: false
    default: "30"
  - name: categories
    description: "Debt categories to include: explicit, structural, duplication, design, all"
    required: false
    default: "all"
  - name: include_trends
    description: Include trend analysis if history available
    required: false
    default: "true"
---

# Technical Debt Report

Generate comprehensive technical debt assessment for: {{.paths}}

## When to Use

- Quarterly technical debt reviews
- Sprint planning to allocate debt paydown time
- Stakeholder reporting on code health
- Before major initiatives to assess baseline

## Workflow

### Step 1: Explicit Debt (SATD)
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: false
```
Find all TODO, FIXME, HACK, BUG markers - debt developers explicitly acknowledged.

### Step 2: Structural Debt
```
analyze_complexity:
  paths: {{.paths}}
  functions_only: true
```
Find overly complex functions that are hard to maintain.

### Step 3: Duplication Debt
```
analyze_duplicates:
  paths: {{.paths}}
  min_lines: 6
  threshold: 0.8
```
Find code clones that require synchronized maintenance.

### Step 4: Design Debt
```
analyze_cohesion:
  paths: {{.paths}}
  sort: lcom
```
Find classes with poor cohesion (doing too many things).

```
analyze_smells:
  paths: {{.paths}}
```
Find architectural smells (cycles, god components, etc.).

### Step 5: Risk Debt
```
analyze_defect:
  paths: {{.paths}}
```
Find high-risk files that likely contain latent bugs.

### Step 6: Knowledge Debt
```
analyze_ownership:
  paths: {{.paths}}
```
Find knowledge silos - code only one person understands.

## Output

### Technical Debt Report

**Scope**: {{.paths}}
**Assessment Date**: [date]
**Categories**: {{.categories}}

---

### Executive Summary

**Overall Debt Level**: [LOW | MODERATE | HIGH | CRITICAL]
**Estimated Remediation**: [effort in person-days]

| Category | Items | Severity Distribution | Trend |
|----------|-------|----------------------|-------|
| Explicit (SATD) | | H/M/L | |
| Structural | | | |
| Duplication | | | |
| Design | | | |
| Risk | | | |
| Knowledge | | | |

### Debt Distribution

```
[Visual distribution across categories]
Explicit:    ████████░░ 40%
Structural:  ██████░░░░ 30%
Duplication: ████░░░░░░ 20%
Design:      ██░░░░░░░░ 10%
```

---

### Explicit Debt (SATD Markers)

Debt that developers explicitly documented:

#### By Severity

| Severity | Count | Examples |
|----------|-------|----------|
| Critical | | Security, data loss risks |
| High | | FIXME, HACK, BUG markers |
| Medium | | TODO with clear action |
| Low | | Notes, optimization ideas |

#### By Category

| Category | Count | Top Files |
|----------|-------|-----------|
| Design | | |
| Defect | | |
| Requirement | | |
| Test | | |
| Security | | |

#### Critical Items (Immediate Attention)

| File | Line | Marker | Content | Age |
|------|------|--------|---------|-----|
| | | | | |

### Structural Debt

Functions that are too complex to maintain safely:

| Function | File | Cyclomatic | Cognitive | Issue |
|----------|------|------------|-----------|-------|
| | | | | Too many branches |
| | | | | Deep nesting |
| | | | | Long method |

**Total**: [count] functions exceed recommended thresholds.

### Duplication Debt

Code that requires synchronized maintenance:

| Metric | Value |
|--------|-------|
| Total clone groups | |
| Duplicated lines | |
| Duplication ratio | |
| Files with clones | |

**Top Clone Groups**:
| Group | Instances | Lines | Maintenance Risk |
|-------|-----------|-------|------------------|
| | | | High - logic duplication |

**Estimated Impact**: Each bug fix in duplicated code requires [N] coordinated changes.

### Design Debt

Classes and modules with structural issues:

#### Low Cohesion Classes

| Class | File | LCOM | WMC | Issue |
|-------|------|------|-----|-------|
| | | | | Should be split |

#### Architectural Smells

| Smell | Count | Components | Impact |
|-------|-------|------------|--------|
| Cyclic Dependencies | | | Build/test complexity |
| God Components | | | Change risk |
| Hub Components | | | Single point of failure |
| Unstable Dependencies | | | Ripple effects |

### Risk Debt

Files with high defect probability (latent bugs):

| File | Probability | Contributing Factors | Action |
|------|-------------|---------------------|--------|
| | | High churn + complexity | Refactor before next change |

**Distribution**:
- High Risk (>0.7): [count] files ([%])
- Medium Risk (0.3-0.7): [count] files ([%])
- Low Risk (<0.3): [count] files ([%])

### Knowledge Debt

Code that only one person understands:

| File/Module | Owner | Lines | Bus Factor Risk |
|-------------|-------|-------|-----------------|
| | | | Critical - no backup |

**Bus Factor Analysis**:
- Single-owner modules: [count]
- Critical paths with bus factor 1: [list]

---

### Remediation Plan

#### Immediate (Block shipping)
| Item | Type | Effort | Impact |
|------|------|--------|--------|
| | | | |

#### This Quarter
| Item | Type | Effort | Impact |
|------|------|--------|--------|
| | | | |

#### Backlog
| Item | Type | Effort | Impact |
|------|------|--------|--------|
| | | | |

### Trend Analysis

| Metric | 30 days ago | Today | Change |
|--------|-------------|-------|--------|
| SATD count | | | |
| Complexity violations | | | |
| Duplication ratio | | | |
| High-risk files | | | |

### Recommendations

1. **Quick Wins** (high impact, low effort):
   - [List items]

2. **Strategic Investments** (high impact, higher effort):
   - [List items]

3. **Preventive Measures**:
   - [Process improvements to prevent debt accumulation]

---

**Next Review**: [suggested date]
