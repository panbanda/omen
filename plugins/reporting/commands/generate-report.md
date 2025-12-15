# Generate Health Report

Generate a complete HTML health report with LLM-generated insights.

## Workflow

1. Check for `omen.toml` or `.omen/omen.toml`. If missing, run `omen-development:setup-config` first.
2. Generate data: `omen report generate --since 1y -o ./omen-report-$(date +%Y-%m-%d)/ .`
3. Create insights dir: `mkdir -p <output-dir>/insights`
4. Spawn analyst agents in parallel (Step 3)
5. Wait for all to complete, then spawn summary agent (Step 4)
6. Validate: `omen report validate -d <output-dir>/`
7. Render: `omen report render -d <output-dir>/ -o report.html`

## Step 3: Spawn Analysts (In Parallel)

Launch all 9 agents simultaneously using the Task tool. Each reads its data file and writes an insight file.

For each agent, provide:
1. The data file path to read
2. The insight file path to write
3. The output format below

---

### Hotspot Analyst
**Read**: `<output-dir>/hotspots.json`
**Write**: `<output-dir>/insights/hotspots.json`

```json
{
  "section_insight": "Narrative about patterns found, specific files, risk levels.",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "Risk level. Why. Action."}
  ]
}
```

---

### SATD Analyst
**Read**: `<output-dir>/satd.json`
**Write**: `<output-dir>/insights/satd.json`

```json
{
  "section_insight": "Narrative about debt patterns, security concerns, age.",
  "item_annotations": [
    {"file": "path/to/file.go", "line": 142, "comment": "Severity. Context. Action."}
  ]
}
```

---

### Ownership Analyst
**Read**: `<output-dir>/ownership.json`
**Write**: `<output-dir>/insights/ownership.json`

```json
{
  "section_insight": "Narrative about bus factor risks, knowledge silos.",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "Risk type. Who owns it. Action."}
  ]
}
```

---

### Duplicates Analyst
**Read**: `<output-dir>/duplicates.json`
**Write**: `<output-dir>/insights/duplication.json`

```json
{
  "section_insight": "Narrative about clone patterns and missing abstractions."
}
```

---

### Churn Analyst
**Read**: `<output-dir>/churn.json`
**Write**: `<output-dir>/insights/churn.json`

```json
{
  "section_insight": "Narrative about churn concentration and instability patterns."
}
```

---

### Cohesion Analyst
**Read**: `<output-dir>/cohesion.json`
**Write**: `<output-dir>/insights/cohesion.json`

```json
{
  "section_insight": "Narrative about god classes and coupling issues.",
  "item_annotations": [
    {"class": "ClassName", "file": "path.go", "wmc": 147, "lcom": 89, "comment": "Issue. Split recommendation."}
  ]
}
```

---

### Flags Analyst
**Read**: `<output-dir>/flags.json`
**Write**: `<output-dir>/insights/flags.json`

```json
{
  "section_insight": "Narrative about stale flags and cleanup priorities.",
  "item_annotations": [
    {"flag": "flag_name", "priority": "CRITICAL", "comment": "Age. Context. Removal steps."}
  ]
}
```

---

### Trends Analyst
**Read**: `<output-dir>/trend.json`, `<output-dir>/score.json`
**Write**: `<output-dir>/insights/trends.json`

```json
{
  "section_insight": "Narrative about trajectory and inflection points.",
  "score_annotations": [
    {"date": "2024-03", "label": "Short label", "change": 8, "description": "What happened"}
  ],
  "historical_events": [
    {"period": "Mar 2024", "change": 8, "primary_driver": "complexity", "releases": ["v2.1.0"]}
  ]
}
```

---

### Components Analyst
**Read**: `<output-dir>/trend.json`, `<output-dir>/cohesion.json`, `<output-dir>/smells.json`, `<output-dir>/score.json`
**Write**: `<output-dir>/insights/components.json`

```json
{
  "component_insights": {
    "complexity": "Narrative about complexity trends.",
    "duplication": "...",
    "coupling": "...",
    "cohesion": "...",
    "smells": "...",
    "satd": "...",
    "tdg": "..."
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

## Step 4: Summary (After All Complete)

Wait for all 9 analysts to finish, then spawn the Summary Analyst.

**Read**: All insight files from `<output-dir>/insights/` plus `<output-dir>/score.json`
**Write**: `<output-dir>/insights/summary.json`

```json
{
  "executive_summary": "## Overview\n\nMarkdown narrative...",
  "key_findings": [
    "**Category**: Specific finding with numbers..."
  ],
  "recommendations": {
    "high_priority": [{"title": "Action", "description": "What, why, impact"}],
    "medium_priority": [...],
    "ongoing": [...]
  }
}
```
