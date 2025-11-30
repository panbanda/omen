---
name: bug-hunt
description: |
  Find the most likely locations for bugs using defect prediction, hotspot analysis, temporal coupling, and ownership patterns. Use this skill when investigating bugs, reviewing high-risk code, or prioritizing testing efforts.
---

# Bug Hunt

Statistically identify the most likely locations for bugs by combining defect prediction models with behavioral signals from git history.

## When to Use

- Investigating a reported bug with unclear location
- Reviewing code for potential issues before release
- Prioritizing code review effort
- Identifying high-risk areas for additional testing

## Prerequisites

Omen must be available as an MCP server. Add to Claude Code settings:

```json
{
  "mcpServers": {
    "omen": {
      "command": "omen",
      "args": ["mcp"]
    }
  }
}
```

## Workflow

### Step 1: Run Defect Prediction

Use the `analyze_defect` tool for statistical defect probability:

```
analyze_defect(paths: ["."], high_risk_only: true)
```

This uses PMAT weights (complexity, churn, coupling) to predict defect likelihood.

### Step 2: Find Hotspots

Use the `analyze_hotspot` tool to find high-churn + high-complexity code:

```
analyze_hotspot(paths: ["."], days: 30)
```

Files that change frequently AND are complex are bug magnets.

### Step 3: Check Temporal Coupling

Use the `analyze_temporal_coupling` tool to find hidden dependencies:

```
analyze_temporal_coupling(paths: ["."], days: 30)
```

Files that always change together may have implicit dependencies that cause bugs.

### Step 4: Review Ownership

Use the `analyze_ownership` tool to find knowledge silos:

```
analyze_ownership(paths: ["."])
```

Files with single owners (bus factor = 1) are higher risk during that person's absence.

## Risk Signals

Combine signals to identify highest-risk areas:

| Signal | Risk Indicator |
|--------|----------------|
| Defect Probability | > 0.7 = high risk |
| Hotspot Score | Top 10% = high risk |
| Temporal Coupling | > 0.8 coupling strength = hidden dependency |
| Bus Factor | = 1 with high complexity = knowledge silo risk |

## Output Format

Present findings as:

```markdown
# Bug Hunt Report

## Highest Risk Files
1. `src/payment/processor.go`
   - Defect probability: 0.85
   - Hotspot rank: #2 (churn: 45, complexity: 28)
   - Temporally coupled with: `src/payment/validator.go` (0.92)
   - Bus factor: 1 (alice@example.com)
   - **Risk**: Changes here often break validator

## Hidden Dependencies
- `auth/session.go` <-> `cache/redis.go` (0.87 coupling)
  - Always change together but no explicit import
  - Likely shares implicit state or assumptions

## Knowledge Silos
- `legacy/importer.go` - Only bob has touched this in 2 years
  - High complexity (cognitive: 45)
  - No recent changes = potentially fragile

## Investigation Priorities
1. Start with X because it has the highest combined risk
2. Check Y for implicit coupling issues
3. Review Z with the domain expert before changes
```

## Bug Investigation Tips

When investigating a specific bug:

1. Check if the bug area appears in defect predictions
2. Look for temporal coupling to find related files to examine
3. Review recent changes in hotspot files
4. Contact the file owner (from ownership analysis) for context
