# Repository Health Report

Generate an interactive HTML repository health report using omen code analysis tools. The report is a single self-contained HTML file with embedded CSS and JavaScript (using Chart.js from CDN).

## Target Repository

This command works with both local and remote repositories:

- **Local**: Run against the current directory (default) or specify a path
- **Remote**: Provide a GitHub repository reference (e.g., `owner/repo`, `https://github.com/owner/repo`, or `owner/repo@branch`)

When a remote repository is specified, omen will clone it to a temporary directory for analysis.

Arguments: $ARGUMENTS

## User Preferences

Before running the analysis, check if the user has specified preferences for trend analysis. If not already specified in the arguments, ask the user **one question at a time** using AskUserQuestion:

### Question 1: Time Range

Ask how far back to analyze for historical trends:

- **Options**: 3 months, 6 months, 1 year (recommended), 2 years, 5 years, 10 years (all available history)
- **Default**: 1 year

### Question 2: Sampling Period

Ask how often to sample the data:

- **Options**: Daily, Weekly, Monthly (recommended)
- **Default**: Monthly

Use the defaults if the user selects them or doesn't have a preference.

## Required Analyses

Run the following omen analyses and incorporate their results:

1. **Repository Score**: `omen score` - Get overall health score and component breakdown
2. **Complexity**: `omen analyze complexity` - Function-level cyclomatic and cognitive complexity
3. **Hotspots**: `omen analyze hotspot` - Files with high churn + high complexity
4. **Churn**: `omen analyze churn` - Recent file change patterns (30 days)
5. **Ownership**: `omen analyze ownership` - Bus factor and knowledge silos
6. **SATD**: `omen analyze satd` - Self-admitted technical debt (TODO/FIXME/HACK)
7. **Duplication**: `omen analyze duplicates` - Code clone detection
8. **Feature Flags**: `omen analyze flags` - Feature flag detection and staleness analysis
9. **Trends**: `omen analyze trend --since 10y --period monthly` - Historical score trends

## Report Structure

### Header Section
- Repository name and generation date
- Large circular score visualization (0-100)
- Key metadata: files analyzed, functions analyzed, threshold status
- Overall trend indicator (improving/declining/stable)

### Component Scores Section
For each component (Complexity, Duplication, Cohesion, TDG, SATD, Coupling, Smells):
- Score with color coding (green >80, yellow 60-80, red <60)
- Progress bar visualization
- Key metrics specific to that component
- Definition box explaining what the metric measures

### Historical Trends Section
- Line chart showing overall score over time with trend line
- Annotations on the chart marking significant score changes (increases and decreases)
- Component trends chart showing individual metrics over time with annotations
- Statistical summary: start score, end score, slope, R-squared
- **Historical Events Table**: For each significant score change (>=2 points):
  - Investigate git history around that date to identify what was released
  - Document the period, magnitude, primary driver (which component), and key releases
  - Color-code rows by severity (red for large drops, green for improvements)
- **Component Events Table**: Major individual component changes that may not have affected overall score
- Insights box explaining patterns (e.g., "decreases correlate with feature releases, increases with cleanup sprints")

### Hotspots Section
- Summary stats: total hotspots, max score, average score
- Table of top 15 hotspots with file path and score
- Definition and importance explanation

### Code Churn Section
- Summary stats: total changes, unique files, lines added/deleted
- Table of highest-churning files
- Top contributors by commit volume

### Ownership Section
- Bus factor score
- Knowledge silo count and ratio
- Chart showing top contributors by code ownership
- Average contributors per file

### SATD Section
- Total items, files affected, critical/high counts
- Doughnut charts for severity and category distribution
- Table listing critical and high-priority items with file locations

### Duplication Section
- Duplication ratio percentage
- Clone group statistics
- Bar chart showing clone groups by size

### Feature Flags Section
- Summary stats: total flags, total references, by-provider breakdown
- Priority distribution doughnut chart (Critical/High/Medium/Low)
- Table of high-priority flags with:
  - Flag key and provider
  - File spread (number of files containing the flag)
  - Days since introduction (staleness indicator)
  - Priority level badge
- Staleness analysis: flags older than expected TTL (14 days for release flags)
- Complexity metrics: average file spread, max nesting depth
- Definition box explaining feature flag hygiene and why stale flags are technical debt

### Recommendations Section
Three cards:
- **High Priority**: Issues needing immediate attention
- **Medium Priority**: Issues to address in upcoming sprints
- **Ongoing Maintenance**: Metrics to monitor

### Glossary Section
Definition boxes for all technical terms used in the report

## Design Requirements

### Visual Style
- Dark theme with GitHub-inspired color palette:
  - Background: #0d1117 (primary), #161b22 (secondary), #21262d (tertiary)
  - Text: #e6edf3 (primary), #8b949e (secondary)
  - Accents: #3fb950 (green), #d29922 (yellow), #f85149 (red), #58a6ff (blue), #a371f7 (purple)
- Cards with subtle borders (#30363d)
- Responsive design with mobile breakpoints

### Charts (Chart.js)
- Use chartjs-plugin-annotation for adding event labels to trend charts
- Consistent color scheme matching the component being visualized
- Tooltips showing detailed values
- Proper axis labels and legends

### Tables
- Hover highlighting
- Badge components for severity levels (critical/high/medium/low)
- Truncated file paths for readability

### Interactive Elements
- Collapsible sections where appropriate
- Tab interfaces for switching between related views

## Historical Event Investigation Process

For each significant score change identified in the trend data:

1. Find the commit SHA and date from the trend analysis
2. Run `git log --oneline --since="<date-1week>" --until="<date+1week>"` to see what was released
3. Look for patterns in branch names (feature/, tech/, refactor/, fix/)
4. Identify the primary component that drove the change
5. Document specific features or cleanup efforts that caused the change
6. Add annotations to both the overall trends chart and component trends chart

## Workflow

### Step 0: Check for Omen Configuration

Before running analyses, check if an omen config file exists in the target repository:

1. Look for `omen.toml` or `.omen/omen.toml` in the repository root
2. If no config file exists:
   - Inform the user: "No omen.toml found. Running setup-config to generate one..."
   - Run the `setup-config` skill to analyze the repository and generate appropriate configuration
   - Wait for setup-config to complete before proceeding
3. If config exists, continue to Step 1

This ensures feature flag providers, exclude patterns, and other project-specific settings are properly configured before analysis.

### Step 1: Determine Target

Parse the argument to determine the target repository:

- If no argument or `.`: analyze current directory
- If a path exists locally: analyze that path
- If `owner/repo` or GitHub URL format: pass directly to omen (it handles cloning)

### Step 2: Gather Analysis Data

Run all required analyses in JSON format. Replace `<target>` with the repository path or GitHub reference:

```bash
# Repository score
omen score <target> -f json > /tmp/score.json

# Complexity analysis
omen analyze complexity <target> -f json > /tmp/complexity.json

# Hotspot analysis
omen analyze hotspot <target> -f json > /tmp/hotspot.json

# Churn analysis (30 days)
omen analyze churn <target> -f json > /tmp/churn.json

# Ownership analysis
omen analyze ownership <target> -f json > /tmp/ownership.json

# SATD analysis
omen analyze satd <target> -f json > /tmp/satd.json

# Duplication analysis
omen analyze duplicates <target> -f json > /tmp/duplicates.json

# Feature flags analysis
omen analyze flags <target> -f json > /tmp/flags.json

# Trend analysis (historical)
omen analyze trend <target> --since 10y --period monthly -f json > /tmp/trend.json
```

Examples:
- Local: `omen score . -f json`
- Remote: `omen score golang/go -f json`
- Remote with ref: `omen score facebook/react@v18.2.0 -f json`

### Step 3: Identify Significant Events

Parse the trend data to find score changes >= 2 points between consecutive periods.

For each significant change:
```bash
git log --oneline --since="<date-1week>" --until="<date+1week>"
```

### Step 4: Generate HTML Report

Create a single self-contained HTML file with:
- All CSS embedded in `<style>` tags
- All JavaScript embedded in `<script>` tags
- Chart.js loaded from CDN: `https://cdn.jsdelivr.net/npm/chart.js`
- Annotation plugin: `https://cdn.jsdelivr.net/npm/chartjs-plugin-annotation`

### Step 5: Save Report

Save as `repository-health-report.html` in the current working directory (or specify output path based on the target repo name for remote repositories, e.g., `golang-go-health-report.html`).

## Output

The final deliverable is a single HTML file that:
- Opens directly in any modern browser
- Requires no server or build step
- Contains all data, styles, and scripts inline
- Prints cleanly for PDF export
