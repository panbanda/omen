# Generate Health Report

Generate a complete HTML health report with LLM-generated insights.

## Workflow

1. Check for `omen.toml` or `.omen/omen.toml`. If missing, run `omen-development:setup-config` first.
2. Generate data: `omen report generate -o ./omen-report-$(date +%Y-%m-%d)/`
3. Create insights dir: `mkdir -p <output-dir>/insights`
4. Spawn analyst agents in parallel (Step 3)
5. Wait for all to complete, then spawn summary agent (Step 4)
6. Validate: `omen report validate -d <output-dir>/`
7. Render: `omen report render -d <output-dir>/ -o report.html`

## Step 3: Spawn Analysts (In Parallel)

Use the Task tool to spawn all 12 agents simultaneously. Each agent reads its data file and writes an insight file with the schema below.

---

**Use the hotspot-analyst agent** to analyze `<dir>/hotspots.json` and write `<dir>/insights/hotspots.json`:
```json
{"section_insight": "string", "item_annotations": [{"file": "path", "comment": "string"}]}
```

---

**Use the satd-analyst agent** to analyze `<dir>/satd.json` and write `<dir>/insights/satd.json`:
```json
{"section_insight": "string", "item_annotations": [{"file": "path", "line": 0, "comment": "string"}]}
```

---

**Use the ownership-analyst agent** to analyze `<dir>/ownership.json` and write `<dir>/insights/ownership.json`:
```json
{"section_insight": "string", "item_annotations": [{"file": "path", "comment": "string"}]}
```

---

**Use the duplicates-analyst agent** to analyze `<dir>/duplicates.json` and write `<dir>/insights/duplication.json`:
```json
{"section_insight": "string"}
```

---

**Use the churn-analyst agent** to analyze `<dir>/churn.json` and write `<dir>/insights/churn.json`:
```json
{"section_insight": "string"}
```

---

**Use the flags-analyst agent** to analyze `<dir>/flags.json` and write `<dir>/insights/flags.json`:
```json
{"section_insight": "string", "item_annotations": [{"flag": "name", "priority": "CRITICAL|HIGH|MEDIUM|LOW", "introduced_at": "ISO8601", "comment": "string"}]}
```

---

**Use the trends-analyst agent** to analyze `<dir>/trend.json` and `<dir>/score.json`, write `<dir>/insights/trends.json`:
```json
{"section_insight": "string", "score_annotations": [{"date": "2024-03", "label": "string", "change": 0, "description": "string"}], "historical_events": [{"period": "Mar 2024", "change": 0, "primary_driver": "complexity", "releases": ["v1.0"]}]}
```

---

**Use the components-analyst agent** to analyze `<dir>/trend.json`, `<dir>/cohesion.json`, `<dir>/smells.json`, `<dir>/score.json` and write `<dir>/insights/components.json`:
```json
{"component_insights": {"complexity": "string", "duplication": "string", "coupling": "string", "cohesion": "string", "smells": "string", "satd": "string", "tdg": "string"}, "component_annotations": {"complexity": [{"date": "2024-03", "label": "string", "from": 0, "to": 0, "description": "string"}]}, "component_events": [{"period": "Mar 2024", "component": "complexity", "from": 0, "to": 0, "context": "string"}]}
```

---

**Use the temporal-analyst agent** to analyze `<dir>/temporal.json` and write `<dir>/insights/temporal.json`:
```json
{"section_insight": "string"}
```

---

**Use the smells-analyst agent** to analyze `<dir>/smells.json` and write `<dir>/insights/smells.json`:
```json
{"section_insight": "string"}
```

---

**Use the graph-analyst agent** to analyze `<dir>/graph.json` and write `<dir>/insights/graph.json`:
```json
{"section_insight": "string"}
```

---

**Use the tdg-analyst agent** to analyze `<dir>/tdg.json` and write `<dir>/insights/tdg.json`:
```json
{"section_insight": "string"}
```

---

## Step 4: Summary (After All Complete)

Wait for all 12 analysts to finish, then:

**Use the summary-analyst agent** to read all `<dir>/insights/*.json` plus `<dir>/score.json` and write `<dir>/insights/summary.json`:
```json
{"executive_summary": "markdown", "key_findings": ["string"], "recommendations": {"high_priority": [{"title": "string", "description": "string"}], "medium_priority": [], "ongoing": []}}
```
