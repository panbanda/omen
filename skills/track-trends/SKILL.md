---
name: track-trends
description: Analyze repository health trends over time using historical git commits. Use for quarterly reports, release retrospectives, or tracking technical debt paydown progress.
---

# Track Trends

Generate historical analysis of repository health metrics over time to identify improvement or degradation patterns.

## Prerequisites

Omen CLI must be installed and available in PATH.

## When to Use

- Quarterly engineering health reports
- Release retrospectives
- Tracking debt paydown initiatives
- Justifying refactoring investments with data
- Identifying gradual quality degradation

## Workflow

### Step 1: Run Trend Analysis

Use the Omen CLI to analyze historical scores:

```bash
# Last 3 months, weekly sampling (default)
omen analyze trend --since 3m

# Last 6 months for quarterly report
omen analyze trend --since 6m --period monthly

# Full year for annual review
omen analyze trend --since 1y --period monthly
```

**Parameters:**
- `--since`: How far back to analyze (3m, 6m, 1y, 2y, 30d, 4w)
- `--period`: Sampling frequency (daily, weekly, monthly)
- `--snap`: Snap to period boundaries (1st of month, Monday)

### Step 2: Interpret Results

The output includes:

| Field | Meaning |
|-------|---------|
| Score | Composite health score (0-100) |
| Slope | Points gained/lost per period |
| R-squared | How consistent the trend is (0-1) |
| Correlation | Direction and strength (-1 to 1) |

**Trend interpretation:**
- Positive slope = improving health
- Negative slope = degrading health
- High R-squared (>0.7) = consistent trend
- Low R-squared (<0.3) = volatile/no clear trend

### Step 3: Analyze Component Trends

Each component has its own trend statistics:

| Component | What It Measures |
|-----------|------------------|
| Complexity | Function complexity violations |
| Duplication | Code clone ratio |
| SATD | Self-admitted technical debt |
| TDG | Technical Debt Gradient |
| Coupling | Cyclic dependencies, instability |
| Smells | Architectural issues |
| Cohesion | Class cohesion (LCOM) |

Look for diverging trends - overall score improving but one component degrading.

## Output Format

Generate a trend report:

```markdown
# Repository Health Trend Report
Period: [Start Date] to [End Date]

## Executive Summary

| Metric | Start | End | Change | Trend |
|--------|-------|-----|--------|-------|
| Overall Score | 72 | 85 | +13 | Improving |
| Complexity | 68 | 91 | +23 | Strong improvement |
| Duplication | 75 | 66 | -9 | Degrading |
| SATD | 80 | 85 | +5 | Stable improvement |
| TDG | 82 | 88 | +6 | Improving |
| Coupling | 70 | 75 | +5 | Stable |
| Smells | 100 | 100 | 0 | Excellent |
| Cohesion | 95 | 100 | +5 | Excellent |

## Trend Analysis

**Overall trajectory:** Score improving at +2.1 points/month (R-squared: 0.89)

**Key observations:**
1. Complexity improved significantly after refactoring sprint in [month]
2. Duplication trending down - clone detection shows new patterns emerging
3. Technical debt (SATD) being addressed consistently

## Component Deep Dive

### Complexity (Strong Improvement)
- Started: 68 (15 functions over threshold)
- Current: 91 (3 functions over threshold)
- Key changes: Refactored payment processor, split auth handler

### Duplication (Concerning)
- Started: 75 (4.2% duplication)
- Current: 66 (6.1% duplication)
- Action needed: New handler patterns being copy-pasted

## Recommendations

### Immediate Actions
1. Address duplication in `handlers/` directory
2. Review new code for copy-paste patterns

### Next Quarter Goals
1. Maintain complexity below 5 violations
2. Reduce duplication back to <5%
3. Continue SATD remediation pace

## Historical Data Points

| Date | Commit | Score | Cx | Dup | SATD | TDG | Coup | Smell | Coh |
|------|--------|-------|-----|-----|------|-----|------|-------|-----|
| 2024-01 | abc123 | 72 | 68 | 75 | 80 | 82 | 70 | 100 | 95 |
| 2024-02 | def456 | 76 | 75 | 72 | 82 | 84 | 72 | 100 | 98 |
| 2024-03 | ghi789 | 85 | 91 | 66 | 85 | 88 | 75 | 100 | 100 |
```

## CI/CD Integration

Add trend checks to release pipelines:

```bash
# Fail if score dropped more than 5 points in last month
omen analyze trend --since 1m --format json | \
  jq -e '.total_change >= -5'

# Generate trend report for release notes
omen analyze trend --since 3m --format markdown > TREND_REPORT.md
```

## Interpreting Patterns

### Healthy Repository
- Stable or improving score over time
- No component consistently degrading
- Occasional dips followed by recovery

### Technical Debt Accumulation
- Gradual score decline (negative slope)
- Multiple components trending down
- SATD count increasing

### Successful Refactoring Initiative
- Score jump at specific point in time
- Sustained improvement after intervention
- TDG and complexity improving together

### Feature Rush Pattern
- Score drops during crunch periods
- Duplication spikes (copy-paste coding)
- Recovery during stabilization sprints
