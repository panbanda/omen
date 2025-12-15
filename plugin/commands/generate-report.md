# Generate Health Report

Generate a complete HTML health report with LLM-generated insights.

## Workflow Overview

1. Check configuration
2. Generate data files with `omen report generate`
3. Analyze data in parallel (spawn subagents for 8 analysis tasks)
3b. Generate executive summary (after all parallel tasks complete)
4. Validate and render HTML

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

#### Task 1: Trends Analysis
- **Input**: `trend.json`, `score.json`
- **Output**: `insights/trends.json`
- **Prompt**: Analyze historical score trajectory. For each significant drop or improvement (5+ points):
  1. Investigate git history for that period to find what changed
  2. Create a score annotation with date, short label, and detailed description
  3. Create a historical event entry with period, change amount, primary driver, and any releases
  Include at least 3-5 annotations for the most significant changes.

#### Task 2: Components Analysis
- **Input**: `trend.json`, `cohesion.json`, `smells.json`, `score.json`
- **Output**: `insights/components.json`
- **Prompt**: Analyze per-component trends (complexity, duplication, coupling, cohesion, smells, satd). For each component:
  1. Write a `component_insights[component]` narrative explaining the trend
  2. Create `component_annotations[component]` entries for significant changes with date, label, from/to scores, and description
  3. Create `component_events` entries for the most impactful changes across all components
  Reference specific commits/PRs when explaining what caused score changes. Identify problematic classes with high LCOM, dangerous hubs, and architectural smells.

#### Task 3: Hotspots Analysis
- **Input**: `hotspots.json`
- **Output**: `insights/hotspots.json`
- **Prompt**: Analyze top hotspots. Identify patterns (are they concentrated in one area?). Write section insight and annotate top files with why they're risky and what to do.

#### Task 4: Technical Debt (SATD) Analysis
- **Input**: `satd.json`
- **Output**: `insights/satd.json`
- **Prompt**: Analyze SATD items by severity and category. Prioritize security-related items. Write section insight and annotate critical items with context.

#### Task 5: Feature Flags Analysis
- **Input**: `flags.json`
- **Output**: `insights/flags.json`
- **Prompt**: Analyze stale feature flags. Identify cleanup candidates. Write section insight about flag hygiene and annotate critical flags with recommended action.

#### Task 6: Ownership Analysis
- **Input**: `ownership.json`
- **Output**: `insights/ownership.json`
- **Prompt**: Analyze bus factor and knowledge silos. Identify files with single-owner risk. Write section insight and annotate high-risk files.

#### Task 7: Duplication Analysis
- **Input**: `duplicates.json`
- **Output**: `insights/duplication.json`
- **Prompt**: Analyze clone patterns. Identify what abstractions are missing. Write section insight about duplication patterns.

#### Task 8: Churn Analysis
- **Input**: `churn.json`
- **Output**: `insights/churn.json`
- **Prompt**: Analyze file change patterns. Identify unstable areas. Write section insight about churn patterns.

### Step 3b: Generate Executive Summary (After Parallel Tasks)

**Wait for all parallel tasks to complete**, then run this final task:

#### Summary & Recommendations
- **Input**: All data files AND all generated insight files from Step 3
- **Output**: `insights/summary.json`
- **Prompt**: Read ALL the generated insight files first. Then synthesize them into:
  1. **Executive Summary** (markdown supported): A comprehensive 2-4 paragraph overview covering:
     - Current health state and trajectory (from trends insight)
     - Most critical risk areas (from hotspots, satd, flags insights)
     - Key architectural concerns (from components insight)
     - Ownership/bus factor risks (from ownership insight)
  2. **Key Findings**: 5-8 specific, actionable findings with file names and numbers. Use markdown for emphasis.
  3. **Recommendations**: Prioritized by urgency. Each recommendation should have a clear title and description with specific files/actions. Use markdown for code references and emphasis.

  The summary should synthesize and reference specific findings from each insight file - not just repeat the data.

### Subagent Instructions Template

Each subagent should:

1. Read the specified input JSON files
2. Analyze patterns and identify the story in the data
3. Write the insight JSON file following the schema below
4. Use analytical language (facts, numbers, specific files)
5. Name specific files/classes, not vague references

### Insight File Schemas

All text fields support **GitHub-flavored markdown** including: bold, italics, code, lists, links, and blockquotes.

**summary.json**
```json
{
  "executive_summary": "## Overview\n\nThe codebase has a **health score of 72**, showing steady improvement over the past 6 months. Key concerns include:\n\n- High complexity in `pkg/analyzer/` (avg cyclomatic: 15)\n- 3 files with bus factor of 1\n- 11 stale feature flags over 2 years old",
  "key_findings": [
    "**Hotspot concentration**: 8 of top 10 hotspots are in `internal/parser/` - consider splitting this package",
    "The `Order` class at `app/models/order.rb` has **LCOM of 147** with 250 methods - a god class"
  ],
  "recommendations": {
    "high_priority": [{"title": "Split parser package", "description": "The `internal/parser/` package has 8 hotspots. Extract language-specific parsers into subpackages: `parser/go/`, `parser/python/`, etc."}],
    "medium_priority": [],
    "ongoing": []
  }
}
```

**trends.json**
```json
{
  "section_insight": "The codebase shows a **gradual improvement trend** from score 65 to 78 over the past year. Major inflection points include the Q2 refactoring sprint and the November performance optimization work.",
  "score_annotations": [
    {"date": "2024-03", "label": "Parser refactor", "change": 8, "description": "Split monolithic `parser.go` into language-specific modules, reducing complexity by 40%"},
    {"date": "2024-06", "label": "Test debt", "change": -5, "description": "Large test refactor introduced 200+ TODO markers that weren't cleaned up"},
    {"date": "2024-09", "label": "Perf sprint", "change": 6, "description": "Commit `abc123f` removed duplicate caching logic across 12 files"}
  ],
  "historical_events": [
    {"period": "Mar 2024", "change": 8, "primary_driver": "complexity", "releases": ["v2.1.0"]},
    {"period": "Jun 2024", "change": -5, "primary_driver": "satd", "releases": []},
    {"period": "Sep 2024", "change": 6, "primary_driver": "duplication", "releases": ["v2.3.0"]}
  ]
}
```

**components.json**
```json
{
  "component_annotations": {
    "complexity": [
      {"date": "2024-03", "label": "Parser split", "from": 72, "to": 85, "description": "Refactored `parser.go` from 2000 lines into 8 focused modules"}
    ],
    "duplication": [
      {"date": "2024-09", "label": "Cache cleanup", "from": 60, "to": 82, "description": "Unified caching logic that was copy-pasted across `pkg/cache/*.go`"}
    ],
    "cohesion": [
      {"date": "2024-06", "label": "Order split", "from": 45, "to": 70, "description": "Extracted pricing logic from `Order` class (LCOM dropped from 147 to 42)"}
    ]
  },
  "component_events": [
    {"period": "Mar 2024", "component": "complexity", "from": 72, "to": 85, "context": "Parser refactoring sprint reduced avg cyclomatic from 18 to 11"},
    {"period": "Sep 2024", "component": "duplication", "from": 60, "to": 82, "context": "Commit `def456` consolidated duplicate error handling"}
  ],
  "component_insights": {
    "complexity": "Complexity has **improved 13 points** since March. The `pkg/analyzer/` directory remains the main concern with 5 functions over cyclomatic 20.",
    "duplication": "Duplication dropped significantly after the September sprint. Remaining clones are mostly in test fixtures (`testdata/`) which are excluded from scoring.",
    "cohesion": "The `Order` class refactoring in June was the biggest win. Two other god classes remain: `UserService` (LCOM 89) and `PaymentProcessor` (LCOM 67)."
  }
}
```

**hotspots.json, satd.json, ownership.json**
```json
{
  "section_insight": "The top 5 hotspots are all in `pkg/parser/` - this package combines **high complexity** (avg cyclomatic 18) with **high churn** (45 commits in 90 days). This concentration suggests the parser abstraction isn't working well.",
  "item_annotations": [
    {"file": "pkg/parser/golang.go", "comment": "**Highest risk file**. 2100 lines with 15 functions over complexity 20. Consider splitting by AST node type: `golang_decl.go`, `golang_expr.go`, `golang_stmt.go`."},
    {"file": "pkg/parser/python.go", "comment": "Second highest. The `parseDecorators` function alone has cyclomatic 35 - extract decorator handling to separate module."}
  ]
}
```

**flags.json**
```json
{
  "section_insight": "Found **11 stale feature flags** over 2 years old. The oldest (`enable_legacy_auth`) dates to 2019 and is referenced in 8 files. These represent cleanup opportunities and potential security risks.",
  "item_annotations": [
    {"flag": "enable_legacy_auth", "priority": "CRITICAL", "introduced_at": "2019-03-15T10:00:00Z", "comment": "**5 years old**, referenced in auth middleware. Likely fully rolled out - verify in LaunchDarkly dashboard then remove."},
    {"flag": "new_pricing_engine", "priority": "HIGH", "introduced_at": "2022-06-01T00:00:00Z", "comment": "2.5 years old, spread across 12 files in `billing/`. Check rollout percentage before cleanup."}
  ]
}
```
Note: Copy `priority` and `introduced_at` from flags.json data (`priority.level` and `staleness.introduced_at` fields)

**duplication.json, churn.json**
```json
{
  "section_insight": "Duplication is concentrated in **error handling patterns** - the same try/catch/log/rethrow pattern appears 23 times across `pkg/api/`. Consider extracting a `handleAPIError()` utility or using middleware."
}
```

## Step 4: Validate

After all analysis tasks complete (including Step 3b):

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
