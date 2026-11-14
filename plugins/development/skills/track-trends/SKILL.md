---
name: track-trends
description: Analyze repository health trends over time using historical git commits. Use for quarterly reports, release retrospectives, or tracking technical debt paydown progress.
---

# Track Trends

Make a historical analysis of the repository health metrics over time. Use it to find patterns of improvement or degradation.

## Prerequisites

The Omen CLI must be installed and available in PATH.

## When to Use

- Quarterly engineering health reports.
- Release retrospectives.
- Debt paydown initiatives that you track.
- Data to justify an investment in refactoring.
- Gradual quality degradation that you must identify.

## Workflow

### Step 1: Run the Trend Analysis

Use the Omen CLI to analyze the historical scores:

```bash
# Last 3 months, weekly sampling (default)
omen score trend --since 3m

# Last 6 months for quarterly report
omen score trend --since 6m --period monthly

# Full year for annual review
omen score trend --since 1y --period monthly
```

**Parameters:**
- `--since`: How far back to analyze (3m, 6m, 1y, 2y, 30d, 4w, all). The default is `all` (the full history).
- `--period`: The sampling frequency (daily, weekly, monthly).

### Step 2: Interpret the Results

The output includes these fields:

| Field | Meaning |
|-------|---------|
| Score | Composite health score (0-100) |
| Slope | Points gained/lost per period |
| R-squared | How consistent the trend is (0-1) |
| Correlation | Direction and strength (-1 to 1) |

**Trend interpretation:**
- A positive slope shows improving health.
- A negative slope shows degrading health.
- A high R-squared (>0.7) shows a consistent trend.
- A low R-squared (<0.3) shows a volatile trend or no clear trend.

### Step 3: Analyze the Component Trends

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

Look for trends that diverge. For example, the overall score improves but one component degrades.

## Output Format

Make a trend report in this format:

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

Add trend checks to the release pipelines:

```bash
# Fail if score dropped more than 5 points in last month
omen -f json score trend --since 1m | \
  jq -e '.total_change >= -5'

# Generate trend report for release notes
omen -f markdown score trend --since 3m > TREND_REPORT.md
```

## Interpret Patterns

### Healthy Repository
- The score is stable or improves over time.
- No component degrades consistently.
- Occasional dips are followed by recovery.

### Technical Debt Accumulation
- The score declines gradually (negative slope).
- Many components trend down.
- The SATD count increases.

### Successful Refactoring Initiative
- The score jumps at a specific point in time.
- The improvement continues after the intervention.
- TDG and complexity improve together.

### Feature Rush Pattern
- The score drops during crunch periods.
- Duplication spikes (copy-paste coding).
- The score recovers during stabilization sprints.
