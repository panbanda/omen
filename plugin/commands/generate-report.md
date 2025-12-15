# Generate Health Report

Generate a complete HTML health report with LLM-generated insights.

## Workflow Overview

1. Check configuration
2. Generate data files with `omen report generate`
3. Analyze data in parallel (spawn subagents)
4. Write insight files
5. Validate and render HTML

## Step 1: Check Configuration

```bash
ls omen.toml .omen/omen.toml 2>/dev/null
```

**If no config exists**, run the `omen:setup-config` skill first. Without configuration:
- Test files inflate complexity/duplication scores
- Generated code (mocks, protobufs) is included
- Feature flag providers won't be detected

## Step 2: Generate Data Files

```bash
omen report generate --since 1y -o ./omen-report-$(date +%Y-%m-%d)/ .
```

This creates these data files in the output directory:

| File | Contains |
|------|----------|
| `metadata.json` | Repository name, timestamp, analysis period |
| `score.json` | Overall health score (0-100) and component breakdown |
| `trend.json` | Historical score data points over time |
| `hotspots.json` | Files with high churn AND high complexity |
| `satd.json` | Self-admitted technical debt (TODO, FIXME, HACK) |
| `cohesion.json` | Class-level CK metrics (LCOM, WMC, CBO, DIT) |
| `flags.json` | Feature flags with staleness analysis |
| `ownership.json` | Code ownership and bus factor |
| `duplicates.json` | Code clone groups |
| `churn.json` | File change frequency |
| `smells.json` | Architectural smells (cycles, hubs, god components) |
| `complexity.json` | Function-level complexity metrics |

## Step 3: Analyze Data (Parallel Subagents)

Create the insights directory:

```bash
mkdir -p <output-dir>/insights
```

**Launch these analysis tasks in parallel using the Task tool.** Each subagent analyzes specific data files and produces one insight file.

### Parallel Analysis Tasks

Launch ALL of these simultaneously:

#### Task 1: Summary & Recommendations
- **Input**: `score.json`, `trend.json`, `hotspots.json`, `satd.json`, `flags.json`, `cohesion.json`
- **Output**: `insights/summary.json`
- **Prompt**: Analyze the overall health score, trend direction, and top issues. Write executive summary, key findings, and prioritized recommendations. Focus on what actions to take.

#### Task 2: Trends Analysis
- **Input**: `trend.json`, `score.json`
- **Output**: `insights/trends.json`
- **Prompt**: Analyze historical score trajectory. Identify significant drops/improvements. Investigate git history for major changes in those periods. Create chart annotations with dates, labels, and descriptions.

#### Task 3: Components Analysis
- **Input**: `trend.json`, `cohesion.json`, `smells.json`, `score.json`
- **Output**: `insights/components.json`
- **Prompt**: Analyze per-component trends. Identify problematic classes (high LCOM), dangerous hubs, and cycles. Create component annotations and events. Use git history to explain what caused major component score changes.

#### Task 4: Hotspots Analysis
- **Input**: `hotspots.json`
- **Output**: `insights/hotspots.json`
- **Prompt**: Analyze top hotspots. Identify patterns (are they concentrated in one area?). Write section insight and annotate top files with why they're risky and what to do.

#### Task 5: Technical Debt (SATD) Analysis
- **Input**: `satd.json`
- **Output**: `insights/satd.json`
- **Prompt**: Analyze SATD items by severity and category. Prioritize security-related items. Write section insight and annotate critical items with context.

#### Task 6: Feature Flags Analysis
- **Input**: `flags.json`
- **Output**: `insights/flags.json`
- **Prompt**: Analyze stale feature flags. Identify cleanup candidates. Write section insight about flag hygiene and annotate critical flags with recommended action.

#### Task 7: Ownership Analysis
- **Input**: `ownership.json`
- **Output**: `insights/ownership.json`
- **Prompt**: Analyze bus factor and knowledge silos. Identify files with single-owner risk. Write section insight and annotate high-risk files.

#### Task 8: Duplication Analysis
- **Input**: `duplicates.json`
- **Output**: `insights/duplication.json`
- **Prompt**: Analyze clone patterns. Identify what abstractions are missing. Write section insight about duplication patterns.

#### Task 9: Churn Analysis
- **Input**: `churn.json`
- **Output**: `insights/churn.json`
- **Prompt**: Analyze file change patterns. Identify unstable areas. Write section insight about churn patterns.

### Subagent Instructions Template

Each subagent should:

1. Read the specified input JSON files
2. Analyze patterns and identify the story in the data
3. Write the insight JSON file following the schema below
4. Use analytical language (facts, numbers, specific files)
5. Name specific files/classes, not vague references

### Insight File Schemas

**summary.json**
```json
{
  "executive_summary": "2-3 sentence state of the codebase",
  "key_findings": ["Finding 1 with specifics", "Finding 2"],
  "recommendations": {
    "high_priority": [{"title": "Action", "description": "Why and how"}],
    "medium_priority": [],
    "ongoing": []
  }
}
```

**trends.json**
```json
{
  "section_insight": "Narrative about codebase evolution",
  "score_annotations": [
    {"date": "2019-03", "label": "Brief label", "change": -10, "description": "What caused this"}
  ],
  "historical_events": [
    {"period": "Sep 2018", "change": -10, "primary_driver": "duplication", "releases": []}
  ]
}
```

**components.json**
```json
{
  "component_annotations": {
    "complexity": [{"date": "2017-08", "label": "Label", "from": 100, "to": 95, "description": "Context"}],
    "duplication": [],
    "coupling": [],
    "cohesion": [],
    "smells": [],
    "satd": [],
    "tdg": []
  },
  "component_events": [
    {"period": "Sep 2018", "component": "duplication", "from": 98, "to": 55, "context": "Why"}
  ],
  "component_insights": {
    "complexity": "Insight about complexity trends",
    "duplication": "Insight about duplication"
  }
}
```

**hotspots.json, satd.json, ownership.json, flags.json**
```json
{
  "section_insight": "Pattern analysis narrative",
  "item_annotations": [
    {"file": "path/to/file.rb", "comment": "Why this matters and what to do"}
  ]
}
```

**duplication.json, churn.json**
```json
{
  "section_insight": "Pattern analysis narrative"
}
```

## Step 4: Validate

After all subagents complete:

```bash
omen report validate -d <output-dir>/
```

This checks:
- All required data files exist and are valid JSON
- All insight files (if present) are valid JSON

## Step 5: Render HTML

```bash
omen report render -d <output-dir>/ -o report.html
```

Open in browser to verify insights render correctly in the report.

## Communication Principles

When writing insights:

1. **Name specific files** - "The Order model at app/models/order.rb" not "some classes"
2. **Include numbers** - "3,000 lines with 250 methods" not "large class"
3. **Explain impact** - "Changes here break 173 dependents" not "high coupling"
4. **Be direct** - If it's a crisis, say "critical" not "could use attention"
5. **Suggest actions** - "Extract pricing logic to OrderPricing" not "consider refactoring"

Use analytical language:
- "Declined from 98 to 55" not "crashed"
- "11 flags over 3 years old" not "many stale flags"
- Reference specific commits/PRs when explaining trend changes
