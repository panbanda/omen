---
name: health-report
title: Health Report
description: Generate an interactive HTML repository health report with visualizations. Use for stakeholder presentations, quarterly reviews, or comprehensive codebase health dashboards.
arguments:
  - name: paths
    description: Paths to analyze
    required: false
    default: "."
  - name: since
    description: "How far back for historical analysis: 3m, 6m, 1y, 2y, all"
    required: false
    default: "1y"
---

# Repository Health Report

Generate a health report for: {{.paths}}

## Quick Start

```bash
# Generate data (runs all analyzers)
omen report generate --since {{.since}} {{.paths}}

# Add insights (optional - see below)

# Render HTML
omen report render -d ./omen-report-<date>/ -o health-report.html
```

## What the Data Tells You

Each JSON file captures a different dimension of codebase health. Here's what to look for and why it matters:

### hotspots.json - Where Bugs Breed

Files that are both complex AND change frequently are your highest-risk code. Microsoft research found this combination is one of the strongest predictors of defects. A complex file nobody touches is stable. A simple file that changes often is easy to work with. But complex + high churn = trouble.

Look for files with scores above 0.6 (critical) or 0.4 (concerning).

### cohesion.json - Classes Trying to Do Everything

When a class has hundreds of methods, depends on dozens of other classes, and its methods don't share common data - that class is trying to do too much. These "god classes" are the #1 architectural problem in most codebases.

Look for classes with high method counts (`nom`), high complexity totals (`wmc`), low cohesion (`lcom` - higher is worse), and high coupling (`cbo`). A class with 250 methods coupling to 700+ classes isn't a class - it's an entire system crammed into one file.

### smells.json - Structural Problems

**Hub components** (`hub_like_dependency`): Code that everything depends on. Change it and you risk breaking everything. The `fan_in` tells you how many dependents - 100+ is a fragility risk.

**Circular dependencies** (`cyclic_dependency`): When A depends on B and B depends on A, you can't safely change either. Filter out vendor code (yarn bundles, node_modules) and focus on application code.

**God components** (`god_component`): Functions/methods with both high fan-in AND fan-out - they touch everything.

### ownership.json - Knowledge Silos

If one person wrote 90% of a critical file, that's organizational risk. "Bus factor" asks how many people would need to leave before this code becomes unmaintainable. Critical code should have 2-3 people who understand it.

### satd.json - Admitted Problems

TODO/FIXME/HACK comments are developers admitting something is wrong. Research shows these often stay for years. Security-related items (SECURITY, VULN) need immediate attention. Known bugs (FIXME, BUG) are defects waiting to happen.

### duplicates.json - Missing Abstractions

Duplicated code means bug fixes need to be applied in multiple places. Look for patterns - if 50 icon components are all duplicated, that's a missing Icon factory component. If API v1 and v2 handlers are duplicated, that's missing shared logic.

### trend.json - Direction of Travel

Is the codebase getting healthier or accumulating debt? The slope tells you: positive = improving, negative = declining. More importantly, which components are driving the change? Stable complexity but declining cohesion means classes are getting bloated over time.

## Adding Insights

Create `insights/` with JSON files. The report incorporates them automatically.

### Writing for Humans

Don't write: "LCOM of 245 indicates poor cohesion"

Write: "The Order class has 250 methods that barely relate to each other. It handles routing, pricing, notifications, and delivery tracking all in one 3,000-line file. Each of these could be its own focused class."

### summary.json

```json
{
  "executive_summary": "State of the codebase, trajectory, biggest issue - in plain language",
  "key_findings": ["Specific finding about specific files"],
  "recommendations": {
    "high_priority": [{"title": "Action", "description": "Why and how"}],
    "medium_priority": [],
    "ongoing": []
  }
}
```

### trends.json

Annotate inflection points - what caused a 10-point drop in 2019?

```json
{
  "section_insight": "Story of how the codebase evolved",
  "score_annotations": [{"date": "2019-03", "label": "Label", "change": -10, "description": "What happened"}]
}
```

### components.json

Name the bloated classes, the dangerous hubs, the cycles. Also analyze each component's trend to identify significant changes (5+ point swings) and **investigate git history** to understand what caused them.

**Analyzing component trends:**

1. Look at `trend.json` points for 5+ point changes between consecutive months
2. For each significant change, investigate what happened:
   ```bash
   # Find commits in that time period
   git log --oneline --since="2018-08-01" --until="2018-10-01"

   # Look for large additions or vendor code
   git log --stat --since="2018-08-01" --until="2018-10-01" | head -100
   ```
3. Correlate with specific commits, PRs, or features added

**Use analytical language:**
- "Declined from 98 to 55" not "crashed" or "collapsed"
- "Increased 20 points" not "recovered" or "improved"
- "Added yarn bundle" not "vendored code explosion"
- State facts: what changed, when, by how much, likely cause

```json
{
  "section_insight": "Root cause of structural issues",
  "component_annotations": {
    "complexity": [
      {"date": "2017-08", "label": "Initial decline", "from": 100, "to": 99, "description": "First complexity threshold exceeded as codebase grew"}
    ],
    "duplication": [
      {"date": "2018-09", "label": "-43 points", "from": 98, "to": 55, "description": "yarn.lock and bundled JS added (commit abc123)"},
      {"date": "2020-04", "label": "+20 points", "from": 69, "to": 89, "description": "Removed duplicate vendor files (PR #456)"}
    ],
    "coupling": [
      {"date": "2021-08", "label": "-12 points", "from": 73, "to": 61, "description": "Ordway billing integration added new dependencies"}
    ],
    "cohesion": [
      {"date": "2023-12", "label": "-11 points", "from": 78, "to": 67, "description": "Order model expanded with delivery tracking methods"}
    ],
    "smells": [],
    "satd": [],
    "tdg": []
  },
  "component_events": [
    {"period": "Sep 2018", "component": "duplication", "from": 98, "to": 55, "context": "Score declined from 93 to 84 due to bundled dependencies"}
  ],
  "problematic_classes": [{"file": "path", "class": "Name", "problem": "Plain explanation", "suggestion": "What to extract"}],
  "dangerous_hubs": [{"component": "path", "dependents": 173, "risk": "Why fragile"}],
  "cycles": [{"files": ["a.js", "b.js"], "issue": "Why problematic"}]
}
```

The `component_annotations` appear as color-coded markers on the component trends chart, matching each component's line color.

### Other insight files

`hotspots.json`, `satd.json`, `ownership.json`, `churn.json`, `duplication.json` - each with `section_insight` and `item_annotations` for files worth calling out.

## Communication Principles

1. **Name specific files** - "The Order model" not "some classes"
2. **Explain why it matters** - "Every change here risks breaking 173 other components"
3. **Give context** - "3,000 lines with 250 methods" not jargon
4. **Be direct** - If it's a crisis, say so
5. **Suggest concrete actions** - "Extract OrderPricing, OrderRouting, OrderNotifications"

## Validation

```bash
omen report validate -d <dir>/
omen report render -d <dir>/ -o report.html
```
