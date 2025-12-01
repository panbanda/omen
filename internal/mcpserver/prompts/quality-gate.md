---
name: quality-gate
title: Quality Gate Check
description: Pass/fail quality gate check against configurable thresholds for complexity, duplication, defect risk, and technical debt
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: cyclomatic_threshold
    description: Max cyclomatic complexity per function
    required: false
    default: "15"
  - name: cognitive_threshold
    description: Max cognitive complexity per function
    required: false
    default: "20"
  - name: duplication_threshold
    description: Max duplication ratio as percentage
    required: false
    default: "5"
  - name: high_risk_threshold
    description: Max percentage of high-risk files
    required: false
    default: "10"
  - name: strict
    description: Fail on any threshold violation
    required: false
    default: "false"
---

# Quality Gate Check

Perform quality gate check for: {{.paths}}

## When to Use

- In CI/CD pipelines before merge
- Before releases
- Periodic quality audits
- To establish baseline metrics

## Workflow

### Step 1: Complexity Check
```
analyze_complexity:
  paths: {{.paths}}
  cyclomatic_threshold: {{.cyclomatic_threshold}}
  cognitive_threshold: {{.cognitive_threshold}}
```
Check for functions exceeding complexity thresholds.

### Step 2: Duplication Check
```
analyze_duplicates:
  paths: {{.paths}}
  min_lines: 6
  threshold: 0.8
```
Measure code duplication ratio.

### Step 3: Defect Risk Check
```
analyze_defect:
  paths: {{.paths}}
```
Count files with high defect probability.

### Step 4: Technical Debt Check
```
analyze_satd:
  paths: {{.paths}}
  strict_mode: true
```
Count critical and high-severity debt markers.

### Step 5: Architectural Smell Check
```
analyze_smells:
  paths: {{.paths}}
```
Detect cyclic dependencies, god components, and other smells.

## Default Thresholds

| Metric | Default | Blocking | Warning |
|--------|---------|----------|---------|
| Max cyclomatic complexity | 15 | > 20 | > 15 |
| Max cognitive complexity | 20 | > 30 | > 20 |
| Duplication ratio | 5% | > 10% | > 5% |
| High-risk files | 10% | > 20% | > 10% |
| Critical SATD items | 0 | > 0 | N/A |
| Cyclic dependencies | 0 | > 0 | N/A |
| God components | 0 | > 1 | > 0 |

## Output

### Quality Gate Report

**Scope**: {{.paths}}
**Status**: [PASS | WARN | FAIL]
**Timestamp**: [datetime]

---

### Gate Results

| Check | Threshold | Actual | Status |
|-------|-----------|--------|--------|
| Max Cyclomatic Complexity | .cyclomatic_threshold | | PASS/WARN/FAIL |
| Max Cognitive Complexity | .cognitive_threshold | | PASS/WARN/FAIL |
| Duplication Ratio | .duplication_threshold% | | PASS/WARN/FAIL |
| High-Risk Files | .high_risk_threshold% | | PASS/WARN/FAIL |
| Critical SATD | 0 | | PASS/FAIL |
| Cyclic Dependencies | 0 | | PASS/FAIL |
| God Components | 0 | | PASS/WARN |

### Complexity Violations

Functions exceeding thresholds:

| Function | File | Cyclomatic | Cognitive | Nesting | Severity |
|----------|------|------------|-----------|---------|----------|
| | | | | | WARN/BLOCK |

### Duplication Report

| Metric | Value |
|--------|-------|
| Total clones | |
| Duplication ratio | |
| Largest clone | |

Top clones to address:
| Clone | Location 1 | Location 2 | Lines |
|-------|-----------|-----------|-------|
| | | | |

### Defect Risk Distribution

| Risk Level | Count | Percentage |
|------------|-------|------------|
| High | | |
| Medium | | |
| Low | | |

High-risk files:
| File | Probability | Primary Factor |
|------|-------------|----------------|
| | | |

### Technical Debt

| Severity | Count | Blocking |
|----------|-------|----------|
| Critical | | Yes |
| High | | No |
| Medium | | No |
| Low | | No |

Critical items (must fix):
| File | Line | Marker | Content |
|------|------|--------|---------|
| | | | |

### Architectural Issues

| Issue | Count | Severity |
|-------|-------|----------|
| Cyclic Dependencies | | BLOCK |
| God Components | | WARN |
| Hub Components | | WARN |
| Unstable Dependencies | | INFO |

### Gate Verdict

**Overall Status**: [PASS | WARN | FAIL]

**Blocking Issues** (must fix before merge):
1. [List blocking items]

**Warnings** (should address):
1. [List warning items]

**Recommendations**:
1. [Prioritized list of improvements]

### Trend (if available)

| Metric | Previous | Current | Change |
|--------|----------|---------|--------|
| Complexity violations | | | |
| Duplication ratio | | | |
| High-risk files | | | |

---

**Exit Code**: [0 = pass, 1 = fail]
