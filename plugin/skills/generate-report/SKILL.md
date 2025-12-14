---
name: generate-report
description: Generate interactive HTML health reports with optional LLM insights. Use for stakeholder presentations, quarterly reviews, or comprehensive codebase health dashboards.
---

# Generate Health Report

Create a self-contained HTML repository health report with all analyzer data and optional LLM-generated insights.

## Prerequisites

1. Omen CLI must be installed and available in PATH
2. **omen.toml must exist** - If not, run the `omen:setup-config` skill first (see Step 0)

## Quick Start

```bash
# 1. Check for config (if missing, run omen:setup-config skill)
ls omen.toml .omen/omen.toml 2>/dev/null || echo "CONFIG MISSING - run omen:setup-config skill first"

# 2. Generate data files
omen report generate --since 1y .

# 3. Render to HTML
omen report render -d ./omen-report-<date>/ -o health-report.html
```

## Full Workflow

### Step 0: Ensure Configuration Exists

Before generating a report, check if `omen.toml` or `.omen/omen.toml` exists in the target repository:

```bash
ls omen.toml .omen/omen.toml 2>/dev/null
```

**If no config exists**, run the `omen:setup-config` skill first. This is critical because without proper configuration:
- Test files will be analyzed (inflating complexity/duplication)
- Generated code will be included (mocks, protobufs)
- Feature flag providers won't be detected
- Language-specific exclusions won't be applied

The `omen:setup-config` skill will:
1. Detect primary languages
2. Find test file patterns to exclude
3. Identify generated code patterns
4. Detect feature flag providers
5. Enable cohesion scoring for OO-heavy codebases

**Do not skip this step** - analyzing without proper exclusions produces misleading metrics.

### Step 1: Generate Data Files

Run all analyzers and output JSON data files:

```bash
omen report generate --since 1y -o ./omen-report-$(date +%Y-%m-%d)/ .
```

**Options:**
- `--since` - How far back for historical analysis (3m, 6m, 1y, 2y, all)
- `-o, --output` - Output directory (default: `./omen-report-<date>/`)

**Generated Files:**
- `metadata.json` - Report metadata
- `score.json` - Overall health score
- `complexity.json` - Function complexity
- `hotspots.json` - High churn + high complexity
- `churn.json` - File change patterns
- `ownership.json` - Bus factor data
- `satd.json` - Technical debt items
- `duplicates.json` - Code clones
- `flags.json` - Feature flags
- `smells.json` - Architectural smells
- `cohesion.json` - LCOM metrics
- `trend.json` - Historical trends

### Step 2: Add LLM Insights (Optional)

Read the generated data files and create insight files in `insights/`:

**Create `insights/summary.json`:**
```json
{
  "executive_summary": "2-3 paragraph overview",
  "key_findings": ["Finding 1", "Finding 2", "Finding 3"],
  "recommendations": {
    "high_priority": [{"title": "...", "description": "..."}],
    "medium_priority": [{"title": "...", "description": "..."}],
    "ongoing": [{"title": "...", "description": "..."}]
  }
}
```

**Create `insights/hotspots.json`:**
```json
{
  "section_insight": "Pattern analysis of hotspot distribution",
  "item_annotations": [
    {"file": "path/to/file.go", "comment": "Context for this hotspot"}
  ]
}
```

**Create `insights/satd.json`:**
```json
{
  "section_insight": "Pattern analysis of debt distribution",
  "item_annotations": [
    {"file": "path/to/file.go", "line": 42, "comment": "Context"}
  ]
}
```

### Step 3: Validate

Validate all files before rendering:

```bash
omen report validate -d ./omen-report-<date>/
```

### Step 4: Render HTML

Generate the final report:

```bash
omen report render -d ./omen-report-<date>/ -o health-report.html
```

### Step 5: Serve for Iteration (Optional)

For live iteration on insights:

```bash
omen report serve -d ./omen-report-<date>/ -p 8080
```

Edit insight files, refresh browser to see changes.

## Report Contents

### Without Insights
- Overall score card
- Component score breakdown
- Hotspots table
- SATD items table
- Trend charts

### With Insights
All of the above, plus:
- Executive summary
- Key findings
- Prioritized recommendations
- Chart annotations
- Item-level commentary

## Use Cases

1. **Quarterly Review**: Generate report before engineering reviews
2. **Release Readiness**: Check health before major releases
3. **New Team Member**: Onboard with codebase overview
4. **Stakeholder Update**: Share with non-technical stakeholders
5. **Refactoring Planning**: Identify priority areas
