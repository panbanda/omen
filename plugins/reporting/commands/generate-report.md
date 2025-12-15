# Generate Health Report

Generate a complete HTML health report with research-backed LLM-generated insights.

This command spawns specialized analyst agents, each trained on the academic research behind their domain (McCabe, Chidamber-Kemerer, Potdar & Shihab, etc.).

## Workflow Overview

1. Check configuration
2. Generate data files with `omen report generate`
3. Analyze data in parallel (spawn specialized analyst agents)
4. Generate executive summary (after all parallel tasks complete)
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

## Step 3: Analyze Data (Specialized Analyst Agents)

Create the insights directory:

```bash
mkdir -p <output-dir>/insights
```

**Launch these analysis tasks in parallel using the Task tool.** Each agent uses a specialized skill trained on the academic research for its domain.

### Parallel Analysis Tasks

Launch ALL of these simultaneously. Each agent should:
1. Read its skill file first (contains research findings and thresholds)
2. Read the specified input JSON files
3. Write the insight JSON file following the skill's output format

| Task | Skill | Input | Output | Research Basis |
|------|-------|-------|--------|----------------|
| Trends | `omen-reporting:trends-analyst` | `trend.json`, `score.json` | `insights/trends.json` | Score trajectory analysis |
| Components | `omen-reporting:components-analyst` | `trend.json`, `cohesion.json`, `smells.json`, `score.json` | `insights/components.json` | Per-component health |
| Hotspots | `omen-reporting:hotspot-analyst` | `hotspots.json` | `insights/hotspots.json` | Tornhill, Nagappan & Ball 2005 |
| SATD | `omen-reporting:satd-analyst` | `satd.json` | `insights/satd.json` | Potdar & Shihab 2014 |
| Flags | `omen-reporting:flags-analyst` | `flags.json` | `insights/flags.json` | Feature flag lifecycle |
| Ownership | `omen-reporting:ownership-analyst` | `ownership.json` | `insights/ownership.json` | Bird et al. 2011 |
| Duplication | `omen-reporting:duplicates-analyst` | `duplicates.json` | `insights/duplication.json` | Juergens et al. 2009 |
| Churn | `omen-reporting:churn-analyst` | `churn.json` | `insights/churn.json` | Nagappan & Ball 2005 |

### Agent Prompt Template

For each agent:

```
You are a specialized analyst. First, read the skill file to understand the research behind your analysis:

Read: plugins/reporting/skills/<analyst-name>/SKILL.md

Then read the data files:
Read: <output-dir>/<input-file>.json

Apply the research-based thresholds and patterns from the skill to analyze the data.
Write your insight file to: <output-dir>/insights/<output-file>.json

Follow the output format specified in your skill.
```

### Step 4: Generate Executive Summary (After Parallel Tasks)

**Wait for all parallel tasks to complete**, then run this final task:

#### Summary & Recommendations
- **Skill**: `omen-reporting:summary-analyst`
- **Input**: All data files AND all generated insight files from Step 3
- **Output**: `insights/summary.json`

The summary analyst reads all insight files and synthesizes them into:
1. **Executive Summary**: 2-4 paragraph overview for stakeholders
2. **Key Findings**: 5-8 specific, actionable discoveries
3. **Recommendations**: Prioritized by urgency (high/medium/ongoing)

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

**hotspots.json, ownership.json**
```json
{
  "section_insight": "The top 5 hotspots are all in `pkg/parser/` - this package combines **high complexity** (avg cyclomatic 18) with **high churn** (45 commits in 90 days). This concentration suggests the parser abstraction isn't working well.",
  "item_annotations": [
    {"file": "pkg/parser/golang.go", "comment": "**Highest risk file**. 2100 lines with 15 functions over complexity 20. Consider splitting by AST node type: `golang_decl.go`, `golang_expr.go`, `golang_stmt.go`."},
    {"file": "pkg/parser/python.go", "comment": "Second highest. The `parseDecorators` function alone has cyclomatic 35 - extract decorator handling to separate module."}
  ]
}
```

**satd.json**
```json
{
  "section_insight": "Found **47 SATD markers** across the codebase. 8 are CRITICAL (FIXME/XXX), concentrated in `pkg/auth/` - these may indicate security concerns that were never addressed.",
  "item_annotations": [
    {"file": "pkg/auth/oauth.go", "line": 142, "comment": "**Security concern**: FIXME about token validation bypass. Investigate immediately - may be a vulnerability."},
    {"file": "pkg/api/handler.go", "line": 89, "comment": "XXX marker about race condition in concurrent requests. Add mutex or use sync.Once."}
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

## Step 5: Validate

After all analysis tasks complete (including Step 4):

```bash
omen report validate -d <output-dir>/
```

This checks:
- All required data files exist and are valid JSON
- All insight files (if present) are valid JSON

## Step 6: Render HTML

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
