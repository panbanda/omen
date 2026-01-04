---
name: focus-review
title: Focus Review
description: Identify high-risk areas in a PR or changeset.
arguments:
  - name: changed_files
    description: Comma-separated list of changed files
    required: true
  - name: paths
    description: Repository root
    required: false
    default: "."
---

# Code Review Focus

Review: {{.changed_files}}

## Workflow

### Step 1: Risk Assessment
```
analyze_defect:
  paths: [{{.changed_files}}]
```
Files with probability > 0.7 need extra scrutiny.

### Step 2: Complexity Check
```
analyze_complexity:
  paths: [{{.changed_files}}]
  functions_only: true
```
- Cyclomatic > 15: Require simplification
- Cognitive > 20: Request refactoring

### Step 3: New Debt
```
analyze_satd:
  paths: [{{.changed_files}}]
  strict_mode: true
```
New HACK/FIXME need justification.

### Step 4: Dead Code
```
analyze_deadcode:
  paths: [{{.changed_files}}]
  confidence: 0.8
```
Changes may orphan code.

## Review Priorities

| Finding | Action |
|---------|--------|
| Defect prob > 0.7 | Senior review |
| Cyclomatic +5 | Simplify |
| New HACK/FIXME | Justify |
| Orphaned function | Remove |

## Comment Template

```
**[PRIORITY]**: [File:Line]
What: [Issue description]
Why: [Risk/impact]
Fix: [Suggestion]
```
