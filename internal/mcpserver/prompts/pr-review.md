---
name: pr-review
title: Pull Request Review Gate
description: CI/CD quality gate for pull requests - evaluate commit risk, complexity delta, duplication, and code quality before merge
arguments:
  - name: changed_files
    description: Comma-separated list of files changed in the PR
    required: true
  - name: paths
    description: Repository root for full context
    required: false
    default: "."
  - name: days
    description: Days of git history for JIT context
    required: false
    default: "90"
  - name: strict
    description: Fail on any warning-level finding
    required: false
    default: "false"
---

# Pull Request Review Gate

Evaluate PR risk before merge for changes to: {{.changed_files}}

## When to Use

- Automated CI/CD pipeline check
- Before approving any PR
- When triaging a large changeset
- To ensure consistent review quality

## Workflow

### Step 1: Commit Risk Analysis (JIT)
```
analyze_changes:
  paths: {{.paths}}
  days: {{.days}}
  high_risk_only: false
```
Score each commit using Just-In-Time defect prediction (Kamei et al. 2013).
Factors: fix patterns, entropy, lines changed, files touched, author experience.

### Step 2: Complexity Delta
```
analyze_complexity:
  paths: [{{.changed_files}}]
  functions_only: true
```
Check if changes increased complexity. Flag functions exceeding thresholds.

### Step 3: Duplication Check
```
analyze_duplicates:
  paths: {{.paths}}
  min_lines: 6
  threshold: 0.8
```
Detect if changes introduced code clones.

### Step 4: Dead Code Check
```
analyze_deadcode:
  paths: [{{.changed_files}}]
  confidence: 0.8
```
Check if changes orphaned any code that should be removed.

### Step 5: Technical Debt Markers
```
analyze_satd:
  paths: [{{.changed_files}}]
  strict_mode: true
```
Check for new TODO/FIXME/HACK markers. Require justification.

### Step 6: File Risk Assessment
```
analyze_defect:
  paths: [{{.changed_files}}]
```
Check if changes touched statistically high-risk files.

## Gate Criteria

| Check | Pass | Warn | Fail |
|-------|------|------|------|
| Max commit risk | < 0.5 | 0.5-0.7 | > 0.7 |
| Complexity delta | < +5 | +5 to +10 | > +10 |
| New code clones | 0 | 1-2 small | >2 or large |
| Orphaned code | 0 | warnings | confirmed |
| New HACK/FIXME | 0 | with ticket | without ticket |
| Security SATD | 0 | N/A | any |
| High-risk file changes | documented | undocumented | critical path |

## Output

### PR Review Gate Report

**PR**: [PR number/title]
**Changed Files**: {{.changed_files}}
**Status**: [PASS | WARN | FAIL]

---

### Gate Summary

| Check | Result | Details |
|-------|--------|---------|
| Commit Risk | PASS/WARN/FAIL | Max: [score], Avg: [score] |
| Complexity | PASS/WARN/FAIL | Delta: [+/-N] |
| Duplication | PASS/WARN/FAIL | [N] clones introduced |
| Dead Code | PASS/WARN/FAIL | [N] items orphaned |
| Tech Debt | PASS/WARN/FAIL | [N] new markers |
| File Risk | PASS/WARN/FAIL | [N] high-risk files touched |

### Commit Analysis

Risk scores for commits in this PR:

| Commit | Message | Risk | Factors | Action |
|--------|---------|------|---------|--------|
| [sha] | | | | |

**High-Risk Commits** (require senior review):
- [List commits with risk > 0.7]

### Complexity Report

Functions with complexity changes:

| Function | File | Before | After | Delta | Status |
|----------|------|--------|-------|-------|--------|
| | | | | | OK/WARN/FAIL |

**Threshold Violations**:
- Cyclomatic > 10: [list]
- Cognitive > 15: [list]
- Nesting > 4: [list]

### Duplication Report

Code clones introduced or expanded:

| Clone | File 1 | File 2 | Lines | Action |
|-------|--------|--------|-------|--------|
| | | | | Extract to shared function |

### Dead Code

Potentially orphaned by these changes:

| Symbol | File | Type | Confidence | Action |
|--------|------|------|------------|--------|
| | | | | Verify/Remove |

### Technical Debt

New debt markers introduced:

| File | Line | Marker | Content | Verdict |
|------|------|--------|---------|---------|
| | | | | OK (has ticket) / FAIL |

**Security markers**: [NONE | list items]

### File Risk Assessment

High-risk files modified in this PR:

| File | Defect Probability | Churn | Review Level |
|------|-------------------|-------|--------------|
| | | | Standard/Enhanced/Critical |

---

### Review Requirements

Based on this analysis:

**Required Reviewers**:
| Reviewer | Reason |
|----------|--------|
| | High-risk commit author |
| | Expert on modified code |

**Review Focus Areas**:
1. [Most critical area to review]
2. [Second area]
3. [Third area]

### Checklist

**Before Approving**:
- [ ] High-risk commits reviewed by senior engineer
- [ ] Complexity increases justified
- [ ] No unexplained code clones
- [ ] Orphaned code confirmed intentional or removed
- [ ] Tech debt markers have tickets
- [ ] No security SATD markers

### Verdict

**Gate Status**: [PASS | WARN | FAIL]

**Blocking Issues** (must fix):
1. [List blocking items]

**Warnings** (should address):
1. [List warning items]

**Recommendation**: [APPROVE | REQUEST CHANGES | NEEDS DISCUSSION]

---

**CI Exit Code**: [0 = pass, 1 = fail]
