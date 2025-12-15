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

Launch all 9 agents simultaneously using the Task tool.

---

### Hotspot Analyst
**Read**: `<dir>/hotspots.json` | **Write**: `<dir>/insights/hotspots.json`

```json
{
  "section_insight": "string - narrative about patterns",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "string"}
  ]
}
```

---

### SATD Analyst
**Read**: `<dir>/satd.json` | **Write**: `<dir>/insights/satd.json`

```json
{
  "section_insight": "string",
  "item_annotations": [
    {"file": "path/to/file.go", "line": 142, "comment": "string"}
  ]
}
```

---

### Ownership Analyst
**Read**: `<dir>/ownership.json` | **Write**: `<dir>/insights/ownership.json`

```json
{
  "section_insight": "string",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "string"}
  ]
}
```

---

### Duplicates Analyst
**Read**: `<dir>/duplicates.json` | **Write**: `<dir>/insights/duplication.json`

```json
{
  "section_insight": "string"
}
```

---

### Churn Analyst
**Read**: `<dir>/churn.json` | **Write**: `<dir>/insights/churn.json`

```json
{
  "section_insight": "string"
}
```

---

### Flags Analyst
**Read**: `<dir>/flags.json` | **Write**: `<dir>/insights/flags.json`

```json
{
  "section_insight": "string",
  "item_annotations": [
    {"flag": "flag_name", "priority": "CRITICAL|HIGH|MEDIUM|LOW", "introduced_at": "ISO8601", "comment": "string"}
  ]
}
```

---

### Trends Analyst
**Read**: `<dir>/trend.json`, `<dir>/score.json` | **Write**: `<dir>/insights/trends.json`

```json
{
  "section_insight": "string",
  "score_annotations": [
    {"date": "2024-03", "label": "string", "change": 8, "description": "string"}
  ],
  "historical_events": [
    {"period": "Mar 2024", "change": 8, "primary_driver": "complexity", "releases": ["v2.1.0"]}
  ]
}
```

---

### Components Analyst
**Read**: `<dir>/trend.json`, `<dir>/cohesion.json`, `<dir>/smells.json`, `<dir>/score.json` | **Write**: `<dir>/insights/components.json`

```json
{
  "component_insights": {
    "complexity": "string",
    "duplication": "string",
    "coupling": "string",
    "cohesion": "string",
    "smells": "string",
    "satd": "string",
    "tdg": "string"
  },
  "component_annotations": {
    "complexity": [{"date": "2024-03", "label": "string", "from": 72, "to": 85, "description": "string"}]
  },
  "component_events": [
    {"period": "Mar 2024", "component": "complexity", "from": 72, "to": 85, "context": "string"}
  ]
}
```

---

### Patterns Analyst
**Read**: All data files | **Write**: `<dir>/insights/patterns.json`

Look for cross-cutting observations that span multiple analysis types.

```json
{
  "patterns": [
    {"category": "string", "insight": "string"}
  ]
}
```

---

## Step 4: Summary (After All Complete)

Wait for all analysts to finish, then spawn the Summary Analyst.

**Read**: All `<dir>/insights/*.json` plus `<dir>/score.json` | **Write**: `<dir>/insights/summary.json`

```json
{
  "executive_summary": "markdown string",
  "key_findings": ["string", "string"],
  "recommendations": {
    "high_priority": [{"title": "string", "description": "string"}],
    "medium_priority": [{"title": "string", "description": "string"}],
    "ongoing": [{"title": "string", "description": "string"}]
  }
}
```
