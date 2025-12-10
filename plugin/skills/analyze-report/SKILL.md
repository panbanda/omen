---
name: analyze-report
description: Analyze Omen report data and generate insights that tell the story of a codebase's health. Surfaces what matters, explains why, and recommends what to do.
---

# Analyze Report Data

You have JSON data files from Omen's analyzers. Your job is to find the story in this data and communicate it clearly to engineering leadership who need to make decisions about where to invest effort.

## What You're Looking For

The research behind these metrics tells us what predicts problems:

### Files That Are Both Complex AND Frequently Changed

These are your highest-risk files. A complex file that nobody touches is fine - leave it alone. A simple file that changes constantly is fine - it's easy to work with. But a complex file that changes often? That's where bugs breed. Microsoft research found this combination is one of the strongest predictors of defects.

In `hotspots.json`, look for files with high scores. The score combines how often the file changes with how complex it is. Anything above 0.6 needs attention. Above 0.4 deserves a closer look.

### Classes That Do Too Much

When a class has hundreds of methods, touches dozens of other classes, and its methods don't share any common data - that's a class trying to do everything. Research calls these "god classes" and they're the #1 source of architectural problems.

In `cohesion.json`, look for classes with:
- Many methods (the `nom` field - "number of methods")
- High complexity (the `wmc` field - sum of all method complexities)
- Methods that don't work together (the `lcom` field - higher means less cohesion)
- Many dependencies on other classes (the `cbo` field - "coupling between objects")

A class with 250 methods and coupling to 700+ other classes isn't a class - it's an entire subsystem crammed into one file. When you find these, name them specifically and explain what responsibilities could be extracted.

### Components Everything Depends On

Some code becomes a "hub" - everything flows through it. Change that code and you risk breaking everything that depends on it. These are change amplifiers.

In `smells.json`, look for `hub_like_dependency` entries. The `fan_in` metric tells you how many other components depend on this one. A component with 100+ dependents is a single point of fragility.

### Circular Dependencies

When A depends on B and B depends on A, you can't change either safely. These cycles make refactoring dangerous and testing difficult.

In `smells.json`, look for `cyclic_dependency` entries. Filter out vendor/bundled code (like yarn bundles or node_modules) - focus on application code.

### Knowledge Concentrated in Too Few People

"Bus factor" asks: how many people would need to leave before this code becomes unmaintainable? If one person wrote 90% of a critical file, that's organizational risk.

In `ownership.json`, look for files where one contributor dominates. Critical code should have at least 2-3 people who understand it.

### Debt People Admitted To

When developers write "TODO: fix this hack later" or "FIXME: this is broken", they're documenting known problems. These comments often stay for years.

In `satd.json`, pay attention to severity:
- Security-related items (SECURITY, VULN, UNSAFE) need immediate attention
- Known bugs (FIXME, BUG) are defects waiting to bite someone
- Design shortcuts (HACK, KLUDGE) indicate structural problems

### Code That Got Copied Instead of Shared

Every time code is duplicated, future bug fixes need to be applied in multiple places. Research shows duplicated code has significantly more bugs because fixes don't get applied consistently.

In `duplicates.json`, look for patterns - are the same types of files being duplicated? Icon components? API handlers? Test utilities? The pattern tells you what abstraction is missing.

### Trends Over Time

Is the codebase getting healthier or accumulating problems? Look at `trend.json` for the slope - positive means improving, negative means declining.

More importantly, look at which components are driving the change. If complexity is stable but cohesion is declining, that tells you classes are getting bloated over time.

## How to Explore the Data

Start with the big picture:
```bash
jq '.score, .components' <dir>/score.json
jq '.slope, .start_score, .end_score' <dir>/trend.json
```

Then dig into problem areas. Filter to application code - ignore vendor bundles, generated code, and test fixtures when looking for architectural issues.

## Writing Insights

Create an `insights/` directory with JSON files. The HTML report will incorporate these.

### summary.json

Tell the story in plain language. Don't say "LCOM of 245 indicates poor cohesion" - say "The Order class has 250 methods that barely relate to each other. It's trying to handle routing, pricing, notifications, and delivery tracking all in one 3,000-line file."

```json
{
  "executive_summary": "What's the state of this codebase? What's the trajectory? What's the single biggest issue?",
  "key_findings": [
    "Plain-language finding about specific files/classes",
    "Another finding with context about why it matters"
  ],
  "recommendations": {
    "high_priority": [
      {"title": "Action", "description": "Why and roughly how"}
    ],
    "medium_priority": [],
    "ongoing": []
  }
}
```

### trends.json

Annotate significant changes in the score history. What happened in March 2019 that caused a 10-point drop? Correlate with the component breakdown.

```json
{
  "section_insight": "The story of how this codebase evolved",
  "score_annotations": [
    {"date": "2019-03", "label": "Brief label", "change": -10, "description": "What drove this change"}
  ]
}
```

### components.json

Explain the architectural issues AND annotate each component's trend with significant changes. **Investigate git history** to understand what caused each change.

**Analyzing component trends:**

1. Look at `trend.json` points for 5+ point changes between consecutive months
2. For each significant change, investigate what happened:
   ```bash
   # Find commits in that time period
   git log --oneline --since="2018-08-01" --until="2018-10-01"

   # Look for large additions
   git log --stat --since="2018-08-01" --until="2018-10-01" | head -100
   ```
3. Correlate with specific commits, PRs, or features

**Use analytical language:**
- "Declined from 98 to 55" not "crashed" or "collapsed"
- "Increased 20 points" not "recovered" or "improved"
- State facts: what changed, when, by how much, likely cause
- Reference specific commits or PRs when possible

```json
{
  "section_insight": "Root cause analysis of structural issues",
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
  "problematic_classes": [
    {"file": "path/to/file", "class": "ClassName", "problem": "Plain language explanation", "suggestion": "What to extract"}
  ],
  "dangerous_hubs": [
    {"component": "path", "dependents": 173, "risk": "Why this is fragile"}
  ],
  "cycles": [
    {"files": ["a.js", "b.js"], "issue": "Why this creates problems"}
  ]
}
```

The `component_annotations` appear as color-coded markers on the chart (green=complexity, blue=duplication, yellow=coupling, purple=cohesion, red=smells).

### Other insight files

Create `hotspots.json`, `satd.json`, `ownership.json`, `churn.json`, `duplication.json` as needed. Each should have a `section_insight` explaining the pattern and `item_annotations` for specific files worth calling out.

## Communication Principles

1. **Name specific files and classes** - "The Order model" not "some classes"
2. **Explain why it matters** - "This makes every change risky" not "high coupling"
3. **Give context** - "3,000 lines with 250 methods" not "high WMC"
4. **Be direct about severity** - If something is a crisis, say so
5. **Suggest concrete next steps** - "Extract pricing logic into OrderPricing" not "refactor"

## Validation

Before finishing:
```bash
omen report validate -d <dir>/
omen report render -d <dir>/ -o report.html
```
