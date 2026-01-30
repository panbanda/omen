---
name: report-debt
description: Generate comprehensive tech debt assessment with quantified metrics and prioritized issues. Use for sprint planning, stakeholder reports, or justifying refactoring time allocation.
---

# Report Debt

Generate a comprehensive technical debt assessment with quantified metrics, prioritized issues, and actionable recommendations.

## Prerequisites

Omen CLI must be installed and available in PATH.

## Workflow

### Step 1: Scan for Self-Admitted Debt

Run the SATD analysis to find explicit debt markers:

```bash
omen -f json satd
```

Captures TODO, FIXME, HACK, XXX, and other debt markers with severity.

### Step 2: Calculate TDG Scores

Run the TDG analysis for quality grades:

```bash
omen -f json tdg
```

TDG provides A-F grades based on complexity, churn, and health metrics.

### Step 3: Measure Duplication

Run the clone detection analysis:

```bash
omen -f json clones
```

Calculate duplication ratio and identify clone clusters.

### Step 4: Assess Defect Risk

Run the defect prediction analysis:

```bash
omen -f json defect
```

Identify files with high predicted defect probability.

### Step 5: Check Complexity

Run the complexity analysis:

```bash
omen -f json complexity
```

Find functions exceeding complexity thresholds.

## Debt Categories

Organize findings by category:

| Category | Source | Impact |
|----------|--------|--------|
| Explicit | SATD markers | Known issues, documented |
| Structural | TDG grade D/F | Hard to maintain |
| Duplication | Clone ratio > 5% | Maintenance burden |
| Risk | Defect probability > 0.7 | Likely to cause bugs |
| Complexity | Cyclomatic > 15 | Hard to test/modify |

## Output Format

Generate a stakeholder-ready report:

```markdown
# Technical Debt Report
Generated: YYYY-MM-DD

## Executive Summary

| Metric | Value | Status |
|--------|-------|--------|
| Overall TDG Grade | C+ | Needs attention |
| Duplication Ratio | 7.2% | Above target (5%) |
| High-Risk Files | 12 | 8% of codebase |
| SATD Items | 45 | 12 critical |

## Debt by Category

### Explicit Debt (SATD)
- **Critical**: 12 items (FIXME, HACK)
- **High**: 18 items (TODO with urgency)
- **Normal**: 15 items (TODO)

Top items:
1. `payment/processor.go:45` - HACK: Temporary fix for race condition
2. `auth/session.go:123` - FIXME: Security review needed
3. ...

### Structural Debt (TDG Grades)
| Grade | Files | % of Codebase |
|-------|-------|---------------|
| A | 45 | 30% |
| B | 52 | 35% |
| C | 32 | 21% |
| D | 15 | 10% |
| F | 6 | 4% |

Worst files:
1. `legacy/importer.go` - Grade F (TDG: 18)
2. `core/processor.go` - Grade D (TDG: 35)

### Duplication Debt
- **Total clones**: 23 clone groups
- **Duplicated lines**: 1,245 (7.2% of codebase)

Largest clones:
1. 85 lines duplicated across 3 files in `handlers/`
2. 45 lines duplicated between `validator.go` and `checker.go`

### Risk Debt
- **High-risk files**: 12 (defect probability > 0.7)
- **Medium-risk files**: 28 (probability 0.5-0.7)

## Recommended Actions

### Sprint 1 (Quick Wins)
1. Address 12 critical SATD items (est: 8 hours)
2. Consolidate largest clone cluster (est: 4 hours)
3. Split `legacy/importer.go` (est: 6 hours)

### Sprint 2-3 (Structural)
1. Refactor Grade D files (est: 20 hours)
2. Reduce complexity in `core/processor.go` (est: 8 hours)

### Long Term
1. Eliminate all Grade F files
2. Reduce duplication to < 3%
3. Add tests to high-risk files

## Tracking

| Metric | Last Month | This Month | Trend |
|--------|------------|------------|-------|
| TDG Average | 62 | 58 | Improving |
| Duplication | 8.1% | 7.2% | Improving |
| SATD Critical | 15 | 12 | Improving |
```
