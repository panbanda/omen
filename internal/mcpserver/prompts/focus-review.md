---
name: focus-review
title: Focus Review
description: Identify high-priority areas to focus on during code review. Use when reviewing large PRs, preparing code for review, or prioritizing review effort based on complexity and risk.
arguments:
  - name: changed_files
    description: Comma-separated list of changed files to review
    required: true
  - name: paths
    description: Repository root for context
    required: false
    default: "."
  - name: days
    description: Days of git history for context
    required: false
    default: "30"
---

# Code Review Focus

Focus your review effort on the highest-risk areas of these changes: {{.changed_files}}

## When to Use

- When reviewing a pull request
- Before approving a merge
- When triaging a large changeset
- To prioritize limited review time

## Workflow

### Step 1: Commit Risk Analysis (JIT)
```
analyze_changes:
  paths: {{.paths}}
  days: {{.days}}
  high_risk_only: false
```
Score commits using Just-In-Time defect prediction (Kamei et al. 2013).
Factors: lines changed, files touched, fix patterns, author experience, entropy.
High-risk commits (>0.7) need senior review.

### Step 2: Complexity Analysis
```
analyze_complexity:
  paths: [{{.changed_files}}]
  functions_only: true
```
Check if changes increased complexity. Flag functions exceeding thresholds.

### Step 3: Defect Risk Assessment
```
analyze_defect:
  paths: [{{.changed_files}}]
```
Check if changes touched high-risk files. Higher scrutiny needed.

### Step 5: Duplication Detection
```
analyze_duplicates:
  paths: {{.paths}}
  min_lines: 6
  threshold: 0.8
```
Check if changes introduced code clones. Suggest extraction.

### Step 6: Dead Code Check
```
analyze_deadcode:
  paths: [{{.changed_files}}]
  confidence: 0.8
```
Check if changes orphaned any code that should be removed.

### Step 7: SATD Markers
```
analyze_satd:
  paths: [{{.changed_files}}]
  strict_mode: true
```
Check for new TODO/FIXME/HACK markers. Should be justified.

### Step 8: Dependency Impact
```
analyze_graph:
  paths: {{.paths}}
  scope: function
  include_metrics: true
```
Understand what depends on the changed code.

## Review Priorities

| Finding | Priority | Action |
|---------|----------|--------|
| Commit risk > 0.7 | Critical | Senior review required |
| Cognitive complexity +10 | Critical | Request simplification |
| New code in high-risk file | Critical | Extra scrutiny |
| Introduced duplication >20 lines | High | Suggest extraction |
| New HACK/FIXME marker | High | Require justification |
| Orphaned code | Medium | Request cleanup |
| New TODO marker | Low | Ensure ticket created |

## Output

### Review Summary

**Files Changed**: {{.changed_files}}
**Overall Risk**: [LOW | MEDIUM | HIGH | CRITICAL]
**Estimated Review Time**: [quick | moderate | thorough]

### Complexity Delta

Functions with significant complexity changes:

| Function | File | Before | After | Delta | Verdict |
|----------|------|--------|-------|-------|---------|
| | | | | | OK/WARN/BLOCK |

**Threshold Violations**:
- Cyclomatic > 10: [list functions]
- Cognitive > 15: [list functions]
- Nesting > 4: [list functions]

### Risk Assessment by File

| File | Defect Probability | Change Risk | Focus Areas |
|------|-------------------|-------------|-------------|
| | | | |

### Duplication Warnings

New or expanded code clones:

| Clone | Location 1 | Location 2 | Lines | Recommendation |
|-------|-----------|-----------|-------|----------------|
| | | | | Extract to shared function |

### Dead Code

Code that may have been orphaned by these changes:

| Item | File | Type | Confidence | Action |
|------|------|------|------------|--------|
| | | function/variable | | Remove/Verify |

### New Technical Debt

SATD markers introduced in this change:

| File | Line | Marker | Content | Requires |
|------|------|--------|---------|----------|
| | | TODO/FIXME/HACK | | Ticket/Justification |

### Dependency Impact

Files that depend on the changed code:

| Changed File | Dependents | Risk if Broken |
|--------------|------------|----------------|
| | | |

### Review Checklist

Priority items to verify during review:

**Must Check** (blocking):
- [ ] [List critical items from above]

**Should Check** (important):
- [ ] [List high-priority items]

**Nice to Check** (if time permits):
- [ ] [List medium-priority items]

### Verdict

**Recommendation**: [APPROVE | REQUEST CHANGES | NEEDS DISCUSSION]

**Key Concerns**:
1. [Most important issue]
2. [Second issue]
3. [Third issue]

**Suggested Improvements** (non-blocking):
- [Optional enhancements]
