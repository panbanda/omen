# Generate Health Report

Generate a complete HTML health report with research-backed LLM-generated insights.

This command spawns specialized analyst agents, each trained on the academic research behind their domain.

## Workflow Overview

1. Check configuration
2. Generate data files with `omen report generate`
3. Analyze data in parallel (spawn 8 specialized analyst agents)
4. Generate executive summary (after all parallel tasks complete)
5. Validate and render HTML

## Step 1: Check Configuration

```bash
ls omen.toml .omen/omen.toml 2>/dev/null
```

**If no config exists**, run the `omen-development:setup-config` skill first.

## Step 2: Generate Data Files

```bash
omen report generate --since 1y -o ./omen-report-$(date +%Y-%m-%d)/ .
```

Then create the insights directory:

```bash
mkdir -p <output-dir>/insights
```

## Step 3: Spawn Analyst Agents (In Parallel)

**Launch ALL 8 of these agents simultaneously using the Task tool.**

Each agent prompt below includes the research basis and output format. Pass the full prompt to each agent.

---

### Agent 1: Hotspot Analyst

**Input**: `<output-dir>/hotspots.json`
**Output**: `<output-dir>/insights/hotspots.json`

**Prompt**:
```
You are a Hotspot Analyst trained on hotspot research.

## Research You Must Apply

**Tornhill's "Your Code as a Crime Scene" (2015)**
- 4-8% of files typically contain the majority of bugs
- Files that are both complex AND frequently modified are highest risk

**Nagappan & Ball 2005 (Microsoft Research)**
- Relative code churn predicts defect density with 89% accuracy
- Files churned more times have higher defect density

**Risk Thresholds**
| Score | Severity | Action |
|-------|----------|--------|
| >= 0.7 | Critical | Prioritize immediately |
| >= 0.5 | High | Schedule for next sprint |
| >= 0.3 | Medium | Monitor actively |

## Your Task

Read: <output-dir>/hotspots.json

Analyze the hotspot data. Look for:
- Hotspot clusters in same directory = architectural problem
- Single mega-hotspot = likely god class
- Top 5% concentration patterns

Write to: <output-dir>/insights/hotspots.json

Output format:
{
  "section_insight": "Narrative referencing research. Be specific: '8 of 10 hotspots are in pkg/parser/' not 'hotspots are concentrated'.",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "**Risk level**. WHY it's risky with numbers. **Action**: specific refactoring suggestion."}
  ]
}
```

---

### Agent 2: SATD Analyst

**Input**: `<output-dir>/satd.json`
**Output**: `<output-dir>/insights/satd.json`

**Prompt**:
```
You are a SATD Analyst trained on self-admitted technical debt research.

## Research You Must Apply

**Potdar & Shihab 2014**
- Identified 62 recurring SATD patterns
- Found SATD in 2.4% to 31.0% of project files
- **Only 26.3% to 63.5% of SATD ever gets resolved**

**Maldonado & Shihab 2015**
- Design debt is most common and most dangerous (42-84% of SATD)
- Categories: design, defect, documentation, requirement, test debt

**Severity Markers**
| Marker | Severity |
|--------|----------|
| FIXME, XXX | Critical - known bug or security issue |
| HACK, KLUDGE | High - workaround that may break |
| TODO | Medium - planned improvement |

## Your Task

Read: <output-dir>/satd.json

Analyze SATD items. Prioritize:
1. Security-related SATD (FIXME in auth/payment code)
2. Age clusters (old SATD = lost context)
3. High-churn files with SATD

Write to: <output-dir>/insights/satd.json

Output format:
{
  "section_insight": "Reference research: 'Per Potdar & Shihab, only 26-63% of SATD gets resolved. Found X markers, Y are security-related.'",
  "item_annotations": [
    {"file": "path/to/file.go", "line": 142, "comment": "**Severity**. Context and why it matters. Category per Maldonado taxonomy."}
  ]
}
```

---

### Agent 3: Ownership Analyst

**Input**: `<output-dir>/ownership.json`
**Output**: `<output-dir>/insights/ownership.json`

**Prompt**:
```
You are an Ownership Analyst trained on code ownership research.

## Research You Must Apply

**Bird et al. 2011 - "Don't Touch My Code!" (FSE)**
- Components with many "minor contributors" (<5% contribution) have MORE defects
- Sweet spot: 2-4 significant contributors per module
- Both single ownership AND fragmented ownership are problematic

**Nagappan et al. 2008**
- Organizational metrics predict defects better than code metrics alone

**Risk Matrix**
| Pattern | Risk |
|---------|------|
| Single owner (>90%) | Bus factor risk |
| 2-4 major contributors | Optimal |
| Many minor contributors | Highest defect correlation |

## Your Task

Read: <output-dir>/ownership.json

Look for:
- Bus factor = 1 on critical files
- Many minors pattern (5+ contributors, each <10%)
- Knowledge silos (entire directories single-owned)

Write to: <output-dir>/insights/ownership.json

Output format:
{
  "section_insight": "Reference Bird et al.: 'X files have bus factor 1. Y files have many-minors pattern which correlates with higher defects.'",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "**Risk type**. Who owns it, why it's risky. **Action**: specific knowledge transfer suggestion."}
  ]
}
```

---

### Agent 4: Duplicates Analyst

**Input**: `<output-dir>/duplicates.json`
**Output**: `<output-dir>/insights/duplication.json`

**Prompt**:
```
You are a Duplicates Analyst trained on code clone research.

## Research You Must Apply

**Juergens et al. 2009 - "Do Code Clones Matter?" (ICSE)**
- **52% of all clones were inconsistently changed**
- **15% of inconsistent changes caused actual faults**
- Found 107 confirmed defects from inconsistent clone modifications

## Your Task

Read: <output-dir>/duplicates.json

Look for:
- Error handling clones (same try/catch repeated)
- Validation clones (same checks in multiple places)
- Cross-package clones (missing shared library)

Write to: <output-dir>/insights/duplication.json

Output format:
{
  "section_insight": "Reference Juergens: 'Per 2009 ICSE study, 52% of clones get inconsistent changes and 15% cause bugs. Found X clone groups. The [pattern] repeated Y times suggests missing [abstraction].'"
}

Name the specific abstraction to extract (e.g., "Extract handleAPIError() to pkg/errors/").
```

---

### Agent 5: Churn Analyst

**Input**: `<output-dir>/churn.json`
**Output**: `<output-dir>/insights/churn.json`

**Prompt**:
```
You are a Churn Analyst trained on code churn research.

## Research You Must Apply

**Nagappan & Ball 2005 (Microsoft Research)**
- Relative code churn predicts defect density with 89% accuracy
- Key metrics:
  - M1: Churned LOC / Total LOC (higher = more defects)
  - M4: Churn count / Files churned (more churn per file = more defects)
- It's not absolute churn, but *relative* churn that matters

## Your Task

Read: <output-dir>/churn.json

Look for:
- High relative churn (churned LOC > 50% of total)
- Concentrated churn (few files with most changes)
- Sustained churn (files changing many consecutive weeks)

Write to: <output-dir>/insights/churn.json

Output format:
{
  "section_insight": "Reference Nagappan & Ball: 'Per their 89% accuracy finding, relative churn is key. Top 10 files account for X% of changes. [File] has churned Y% of its LOC - strong defect predictor.'"
}
```

---

### Agent 6: Cohesion Analyst (CK Metrics)

**Input**: `<output-dir>/cohesion.json`
**Output**: `<output-dir>/insights/cohesion.json`

**Prompt**:
```
You are a Cohesion Analyst trained on CK metrics research.

## Research You Must Apply

**Chidamber & Kemerer 1994**
- Foundational OO metrics paper (thousands of citations)

**Basili et al. 1996**
- WMC and CBO strongly correlate with fault-proneness

**Thresholds**
| Metric | Threshold | Meaning |
|--------|-----------|---------|
| WMC | < 20 | Sum of method complexities |
| CBO | < 10 | Coupling between objects |
| LCOM | < 3 | Lack of cohesion |
| DIT | < 5 | Inheritance depth |

**God Class Pattern**: High WMC + High LCOM = class doing unrelated things

## Your Task

Read: <output-dir>/cohesion.json

Look for:
- God classes (WMC > 50, LCOM > 10)
- Coupling bombs (CBO > 15)
- Multi-metric violations

Write to: <output-dir>/insights/cohesion.json

Output format:
{
  "section_insight": "Reference C&K: 'Per Chidamber & Kemerer thresholds, X classes exceed limits. The [Class] has WMC Y (Zx threshold), indicating god class.'",
  "item_annotations": [
    {"class": "ClassName", "file": "path.go", "wmc": 147, "lcom": 89, "comment": "**God class**. Metrics and why. **Split into**: specific classes."}
  ]
}
```

---

### Agent 7: Flags Analyst

**Input**: `<output-dir>/flags.json`
**Output**: `<output-dir>/insights/flags.json`

**Prompt**:
```
You are a Feature Flags Analyst.

## Flag Lifecycle

| Age | Status | Action |
|-----|--------|--------|
| < 30 days | Active | Normal |
| 30-90 days | Maturing | Review rollout |
| 90-180 days | Stale | Cleanup candidate |
| 180+ days | Debt | Priority cleanup |
| 2+ years | Critical | Remove immediately |

## Your Task

Read: <output-dir>/flags.json

Look for:
- Ancient flags (> 2 years old)
- Security context flags (in auth/payment code)
- High reference count (in 10+ files)

Write to: <output-dir>/insights/flags.json

Output format:
{
  "section_insight": "Found X flags. Y are over 2 years old. The oldest (flag_name from YYYY) is in [context] - [risk assessment].",
  "item_annotations": [
    {"flag": "flag_name", "priority": "CRITICAL", "introduced_at": "ISO date from data", "comment": "**Age**. Where it's used. **Action**: verification step then removal."}
  ]
}
```

---

### Agent 8: Trends Analyst

**Input**: `<output-dir>/trend.json`, `<output-dir>/score.json`
**Output**: `<output-dir>/insights/trends.json`

**Prompt**:
```
You are a Trends Analyst focused on code health evolution.

## Your Task

Read: <output-dir>/trend.json and <output-dir>/score.json

For each significant score change (5+ points):
1. Identify WHEN it happened (exact month)
2. Investigate git history for that period
3. Determine WHICH component drove the change
4. Note any correlated releases

Write to: <output-dir>/insights/trends.json

Output format:
{
  "section_insight": "Trajectory narrative. 'Score improved from X to Y over Z months. Major inflection at [date] when [what happened].'",
  "score_annotations": [
    {"date": "2024-03", "label": "Short label", "change": 8, "description": "What happened, reference commits if possible"}
  ],
  "historical_events": [
    {"period": "Mar 2024", "change": 8, "primary_driver": "complexity", "releases": ["v2.1.0"]}
  ]
}
```

---

### Agent 9: Components Analyst

**Input**: `<output-dir>/trend.json`, `<output-dir>/cohesion.json`, `<output-dir>/smells.json`, `<output-dir>/score.json`
**Output**: `<output-dir>/insights/components.json`

**Prompt**:
```
You are a Components Analyst focused on per-component health trends.

## Components to Analyze

- Complexity, Duplication, Coupling, Cohesion, Smells, SATD, TDG

## Your Task

Read the input files.

For each component:
1. Current state and trend direction
2. Major events that caused changes
3. Remaining issues
4. Specific recommendations

Write to: <output-dir>/insights/components.json

Output format:
{
  "component_insights": {
    "complexity": "Narrative with numbers. 'Improved 13 points since March. The [file] refactor was the win. Remaining concern: [specific files].'",
    "duplication": "...",
    ...
  },
  "component_annotations": {
    "complexity": [{"date": "2024-03", "label": "Label", "from": 72, "to": 85, "description": "What happened"}]
  },
  "component_events": [
    {"period": "Mar 2024", "component": "complexity", "from": 72, "to": 85, "context": "Explanation"}
  ]
}
```

---

## Step 4: Generate Executive Summary

**Wait for all 8 agents to complete**, then spawn this final agent:

### Agent 10: Summary Analyst

**Input**: All data files AND all insight files from Step 3
**Output**: `<output-dir>/insights/summary.json`

**Prompt**:
```
You are the Summary Analyst. Your role is to synthesize all analysis insights for stakeholders.

## Your Task

Read ALL insight files:
- insights/hotspots.json
- insights/satd.json
- insights/ownership.json
- insights/duplication.json
- insights/churn.json
- insights/cohesion.json
- insights/flags.json
- insights/trends.json
- insights/components.json

Also read: score.json for the current health score.

Synthesize into:

1. **Executive Summary** (2-4 paragraphs, markdown):
   - Current health state and what it means
   - Trajectory (improving/declining)
   - Top 2-3 risks (reference specific findings)
   - Path forward

2. **Key Findings** (5-8 items):
   - Start with category in bold
   - Include specific numbers and file names
   - Be actionable

3. **Recommendations**:
   - high_priority: Security issues, bus factor = 1, critical hotspots
   - medium_priority: God classes, duplication patterns
   - ongoing: Continuous improvement items

Write to: <output-dir>/insights/summary.json

Output format:
{
  "executive_summary": "## Overview\n\nMarkdown content...",
  "key_findings": ["**Category**: Specific finding with numbers..."],
  "recommendations": {
    "high_priority": [{"title": "Action", "description": "What, why, impact"}],
    "medium_priority": [...],
    "ongoing": [...]
  }
}
```

---

## Step 5: Validate

```bash
omen report validate -d <output-dir>/
```

## Step 6: Render HTML

```bash
omen report render -d <output-dir>/ -o report.html
```
