---
name: test-targeting
title: Test Targeting
description: Identify files and functions most needing test coverage based on defect risk, complexity, and churn
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: days
    description: Days of git history for churn analysis
    required: false
    default: "30"
  - name: coverage_threshold
    description: Minimum acceptable coverage percentage
    required: false
    default: "80"
  - name: max_items
    description: Maximum items to return
    required: false
    default: "20"
---

# Test Targeting

Identify where to invest test coverage in: {{.paths}}

## When to Use

- When prioritizing test writing effort
- Before releases to identify coverage gaps
- When coverage budget is limited
- To maximize defect detection per test

## Workflow

### Step 1: Defect Risk Assessment
```
analyze_defect:
  paths: {{.paths}}
```
High-risk files statistically contain more bugs - they need more tests.

### Step 2: Hotspot Analysis
```
analyze_hotspot:
  paths: {{.paths}}
  days: {{.days}}
  top: {{.max_items}}
```
High-churn + high-complexity files need regression protection.

### Step 3: Complexity Analysis
```
analyze_complexity:
  paths: {{.paths}}
  functions_only: true
```
High cyclomatic complexity = many paths = need more test cases.

### Step 4: Knowledge Silos
```
analyze_ownership:
  paths: {{.paths}}
```
Single-owner files need tests as documentation - the tests explain behavior when the owner is unavailable.

### Step 5: SATD Markers
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: false
```
Find test-related debt markers (TODO: add tests, FIXME: test fails).

## Test Priority Factors

| Factor | Weight | Rationale |
|--------|--------|-----------|
| High defect probability | Critical | Statistically likely to have bugs |
| High cyclomatic complexity | High | Many code paths to cover |
| High churn rate | High | Needs regression protection |
| Single owner | Medium | Tests as documentation |
| No existing tests | Medium | Coverage gap |
| Critical business logic | High | High impact if broken |

## Test Case Estimation

Based on cyclomatic complexity (McCabe):
- Complexity 1-5: 3-5 test cases
- Complexity 6-10: 6-10 test cases
- Complexity 11-20: 11-20 test cases
- Complexity 20+: Consider refactoring first

## Output

### Test Targeting Report

**Scope**: {{.paths}}
**History Window**: {{.days}} days
**Coverage Target**: {{.coverage_threshold}}%

---

### Executive Summary

| Priority | Files | Estimated Test Cases |
|----------|-------|---------------------|
| Critical | | |
| High | | |
| Medium | | |
| Total | | |

### Critical Coverage Gaps

Files with high risk and likely insufficient coverage:

| File | Defect Probability | Hotspot Score | Complexity | Priority |
|------|-------------------|---------------|------------|----------|
| | | | | CRITICAL |

### Test Investment Priorities

Ranked by expected defect detection value:

| Rank | File | Risk Score | Est. Test Cases | ROI |
|------|------|------------|-----------------|-----|
| 1 | | | | High |
| 2 | | | | High |
| 3 | | | | Medium |

### Complexity Breakdown

Functions requiring thorough path coverage:

| Function | File | Cyclomatic | Min Test Cases | Coverage Strategy |
|----------|------|------------|----------------|-------------------|
| | | | | Branch coverage |
| | | | | Boundary testing |
| | | | | Error path testing |

### Regression Test Priorities

High-churn files needing change detection tests:

| File | Commits ({{.days}}d) | Churn Score | Test Strategy |
|------|---------------------|-------------|---------------|
| | | | Snapshot/golden tests |
| | | | Integration tests |

### Documentation Tests

Single-owner files where tests serve as documentation:

| File | Owner | Lines | Documentation Need |
|------|-------|-------|-------------------|
| | | | High - explains behavior |

### Test Debt

Existing test-related technical debt:

| File | Line | Marker | Issue |
|------|------|--------|-------|
| | | TODO | Missing test |
| | | FIXME | Flaky test |
| | | HACK | Test workaround |

---

### Testing Strategy by Area

#### Unit Tests Needed
| File/Function | Reason | Approach |
|---------------|--------|----------|
| | High complexity | Path coverage |

#### Integration Tests Needed
| Component | Reason | Approach |
|-----------|--------|----------|
| | Cross-module | End-to-end flow |

#### Regression Tests Needed
| Area | Reason | Approach |
|------|--------|----------|
| | High churn | Golden files/snapshots |

### Test Case Estimates

| File | Cyclomatic Sum | Min Cases | Effort (hours) |
|------|----------------|-----------|----------------|
| | | | |
| **Total** | | | |

### Coverage Investment Plan

#### This Sprint
- [ ] [Highest priority file] - [N] test cases
- [ ] [Second file] - [N] test cases

#### Next Sprint
- [ ] [Medium priority items]

#### Backlog
- [ ] [Lower priority items]

---

### Metrics to Track

After implementing tests:
- Coverage increase per file
- Defect escape rate
- Test execution time
- Flaky test rate

**Next Steps**: After writing tests, use `quality-gate` to verify coverage meets thresholds.
